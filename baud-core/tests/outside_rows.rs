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

/// Rows 9 and 10 from column 55 and 20, seven columns each, and the `CPR`,
/// after `then` and ten characters: what a write back inside the region
/// leaves when it wraps at the right margin to the left margin of row 10.
fn back_inside(then: &str) -> (String, String, String) {
    let (s, reply) = after(&format!("{}{MARGINS}{then}0123456789", horizontal()));
    (at(&s, 9, 55, 7), at(&s, 10, 20, 7), reply)
}

/// The same three, as [`back_inside`] expects them.
fn wrapped_at_the_margin(nine: &str, ten: &str) -> (String, String, String) {
    (nine.to_string(), ten.to_string(), cpr(10, 26))
}

/// **Back inside the region the side margins bound a write again**, however
/// the cursor came there from a row outside it: `LF` from row 8 onto row 9,
/// the region's top, carried by the row's mark (`baud_domain::RegionTop`);
/// `CUP` from row 8, after a write there, onto row 12; `RI` from row 17
/// onto row 16, the region's bottom; and `?1049h` onto the alternate
/// screen, whose row 9 is blank. Every write wraps at the right margin to
/// the left margin of the next line, as the rows inside do above. A fence
/// left at the line's end would carry the write past the margin inside the
/// region. The erases that fill row 9 whole from above it each have a test
/// of their own below, so a red names the one that broke.
#[test]
fn a_write_back_inside_the_region_keeps_the_side_margins() {
    assert_eq!(
        back_inside("\x1b[8;56H\n"),
        wrapped_at_the_margin("I01234I", "J56789J"),
        "LF from row 8"
    );
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[8;1Hx\x1b[12;56H0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 12, 55, 7), at(&s, 13, 20, 7), reply),
        ("L01234L".to_string(), "M56789M".to_string(), cpr(13, 26)),
        "CUP from row 8"
    );
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[?1049h\n"),
        wrapped_at_the_margin(" 01234 ", " 56789 "),
        "?1049h"
    );
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[17;56H\x1bM0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 15, 55, 7), at(&s, 16, 20, 7), reply),
        ("P01234O".to_string(), "P56789 ".to_string(), cpr(16, 26)),
        "RI from row 17"
    );
}

/// **A move from inside the region onto a row outside it takes that row's
/// fence, the line's end**: `LF` from row 8 onto row 9, the region's top,
/// brings the fence to the right margin, and `CUP` back onto row 8 takes it
/// to the line's end again (`Paint::set_row`), so ten characters from
/// column 56 land on 56 to 65 (p. 5-139). A fence kept from row 9 would
/// wrap them at the margin, where `CPR` would read 9;26.
#[test]
fn a_move_from_inside_the_region_onto_a_row_outside_writes_to_the_line_end() {
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[8;1H\n\x1b[8;56H0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 12), reply),
        ("H0123456789H".to_string(), cpr(8, 66))
    );
}

/// **`ED 0` from row 8 fills the region's top to its end, and a line feed
/// onto it still finds the right margin**: the fill builds row 9 a fresh
/// mark, and the erase marks the area's top again (`Paint::erase_display`).
#[test]
fn ed_from_above_the_region_keeps_its_top_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[J\n"),
        wrapped_at_the_margin(" 01234 ", " 56789 ")
    );
}

/// **`EL 2` on row 9, then a move onto row 8**: the erase builds row 9 a
/// fresh mark, the move takes row 8's fence, the line's end, and marks the
/// area's top again (`Paint::set_row`), so the line feed back onto row 9
/// finds the margin. A move that took no fence at all would leave row 9's
/// margin on row 8 and land back on it unseen here: the moves' tests above
/// hold that half.
#[test]
fn el_on_the_region_top_then_a_move_above_keeps_it_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[9;1H\x1b[2K\x1b[8;56H\n"),
        wrapped_at_the_margin(" 01234 ", "J56789J")
    );
}

/// **`EA` from row 8, ECMA-48's ERASE IN AREA (8.3.38) with no area
/// guarded, the page from the cursor on**: it fills row 9 to its end, and
/// marks the area's top again (`Paint::erase_in_area`).
#[test]
fn ea_from_above_the_region_keeps_its_top_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[O\n"),
        wrapped_at_the_margin(" 01234 ", " 56789 ")
    );
}

/// **`DECERA` over row 9 from row 8**: the rectangle runs to the row's end
/// and marks the area's top again (`rect::area_written`).
#[test]
fn decera_over_the_region_top_keeps_it_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[9;1;9;80$z\n"),
        wrapped_at_the_margin(" 01234 ", "J56789J")
    );
}

/// **`DECFRA` over row 9 from row 8**, filled with `.`: as `DECERA`.
#[test]
fn decfra_over_the_region_top_keeps_it_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[46;9;1;9;80$x\n"),
        wrapped_at_the_margin(".01234.", "J56789J")
    );
}

/// **`DECCRA` from row 1 onto row 9, from row 8**: the copy runs to the
/// row's end, as `DECERA`.
#[test]
fn deccra_onto_the_region_top_keeps_it_writing_to_the_margin() {
    assert_eq!(
        back_inside("\x1b[8;56H\x1b[1;1;1;80;1;9;1;1$v\n"),
        wrapped_at_the_margin("A01234A", "J56789J")
    );
}

/// **A scroll or shift of the region with the cursor on the row above it,
/// then a line feed onto the region's top**: `SU` and `SD` move the
/// rectangle's rows and keep each row's word, `SL` and `SR` its columns,
/// and `DECIC` and `DECDC` do nothing outside the margins (`CSI ' }`'s row
/// of `docs/vt-coverage.md`). None moves the cursor, so the line feed lands
/// on row 9 still marked as the area's top (`baud_domain::RegionTop`), and
/// the write wraps at the right margin. Rows 9 and 10 show what each moved:
/// `SU` brings `J` up and `K` into row 10, `SD` blanks row 9's rectangle and
/// moves `I` down into row 10, the rest leave the pattern.
#[test]
fn a_scroll_or_shift_from_above_the_region_keeps_its_top_writing_to_the_margin() {
    let cases = [
        ("\x1b[S", "J01234I", "J56789K"),
        ("\x1b[T", " 01234I", "J56789I"),
        ("\x1b[ @", "I01234I", "J56789J"),
        ("\x1b[ A", "I01234I", "J56789J"),
        ("\x1b['}", "I01234I", "J56789J"),
        ("\x1b['~", "I01234I", "J56789J"),
    ];
    for (op, nine, ten) in cases {
        assert_eq!(
            back_inside(&format!("\x1b[8;56H{op}\n")),
            wrapped_at_the_margin(nine, ten),
            "{op:?}"
        );
    }
}

/// The `CPR` after the margins are set, the cursor put on row `r`, a
/// resize to `cols` columns and `then`, on a blank screen with room for
/// 100 columns.
fn resized(r: usize, cols: u16, then: &str) -> String {
    let room = WinSize {
        ws_row: 24,
        ws_col: 100,
    };
    let to = WinSize {
        ws_row: 24,
        ws_col: cols,
    };
    let mut owned = Owned::new(&room);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, format!("{MARGINS}\x1b[{r};56H").as_bytes());
    assert_eq!(t.resize(&to), Resized::Ok, "{cols} columns");
    support::feed(&mut t, format!("{then}\x1b[6n").as_bytes());
    String::from_utf8_lossy(t.reply()).into_owned()
}

/// **A resize with the margins set puts them at the page's edges and takes
/// the cursor's fence at the new width**: from row 8, outside the region,
/// whose fence was column 80, and from row 12, inside it, whose fence was
/// the margin at 60. At 70 columns ten characters from column 66 wrap at
/// column 70 to column 1 of the next row; at 100 ten from column 76 run to
/// column 85. A fence kept at column 80 would carry the write past the
/// narrowed line. **Set again, the margins split the rows again**: row 8,
/// outside the new region, writes to the line's end, and row 12, inside it,
/// wraps at the margin.
#[test]
fn a_resize_with_margins_set_takes_the_fence_at_the_new_width() {
    for r in [8, 12] {
        assert_eq!(
            resized(r, 70, &format!("\x1b[{r};66H0123456789")),
            cpr(r + 1, 6),
            "row {r}, 70 columns"
        );
        assert_eq!(
            resized(r, 100, &format!("\x1b[{r};76H0123456789")),
            cpr(r, 86),
            "row {r}, 100 columns"
        );
    }
    for cols in [70, 100] {
        let again = "\x1b[9;16r\x1b[21;60s";
        assert_eq!(
            resized(8, cols, &format!("{again}\x1b[8;56H0123456789")),
            cpr(8, 66),
            "row 8, {cols} columns, the margins set again"
        );
        assert_eq!(
            resized(12, cols, &format!("{again}\x1b[12;56H0123456789")),
            cpr(13, 26),
            "row 12, {cols} columns, the margins set again"
        );
    }
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

/// **DECFI and DECBI step from the column a write on the margin left the
/// cursor on**: on row 8, outside the region, `01234` from column 56 ends
/// on the margin, column 60, and the Active Position advances to 61 (p.
/// 5-139, "If the Active Position is outside the Scroll Area, it will
/// advance to the last column of the Active Line"), which `CPR` names
/// (`a_write_ending_on_the_margin_outside_the_vertical_margins_is_one_past_it`).
/// DECFI's algorithm tests that column against the margin and nothing
/// else, `if (active_position.column == right_margin) scroll_left(...)
/// else if (active_position.column < columns_per_page)
/// active_position.column += 1` (p. 5-38), and DECBI's against the left
/// margin (p. 5-39): from 61, outside the side margins, "the active
/// position moves forward one column" to 62, or back one to 60, and
/// nothing shifts. So the cursor is read against the fence the write
/// left it by, the line's end there (`rect::index_from`); read against the
/// right margin, column 61 would be a wrap waiting on 60, and DECFI would
/// shift the region left from it, where `CPR` reads 8;60 and row 12's
/// column 60 is blank, and DECBI would step to 59.
#[test]
fn decfi_and_decbi_after_a_write_ending_on_the_margin_outside_step_from_the_column_past_it() {
    let (s, reply) = after(&format!("{}{MARGINS}\x1b[8;56H01234\x1b9", horizontal()));
    assert_eq!(
        (at(&s, 8, 55, 8), at(&s, 12, 60, 1), reply),
        ("H01234HH".to_string(), "L".to_string(), cpr(8, 62)),
        "DECFI"
    );
    let (s, reply) = after(&format!("{}{MARGINS}\x1b[8;56H01234\x1b6", horizontal()));
    assert_eq!(
        (at(&s, 8, 55, 8), at(&s, 12, 21, 1), reply),
        ("H01234HH".to_string(), "L".to_string(), cpr(8, 60)),
        "DECBI"
    );
}

/// The forms a move's body may not name: the stored fence, the write
/// fence's accessors and type, and the right margin itself.
const NOT_IN_A_MOVE: [&str; 5] = [
    "cold.fence",
    "cold.right",
    "line_fence(",
    "write_fence(",
    "WriteFence",
];

/// **No move reads the write fence, and none takes its stop from the
/// stored fence or the right margin but through the move fence**: the
/// bodies of `CUF`'s, `CUB`'s, `HT` and `CHT`'s, DECFI's and DECBI's
/// functions, and `at_or_inside_right`, which `CUF` and `HT` split on, name
/// none of [`NOT_IN_A_MOVE`]. Each takes its stop from the move fence
/// (`line_move_fence`, `move_stop`), the placed tag from `place`, and the
/// cursor's column through a report's reading. `CUF`'s arm under DECLRMM
/// read the right margin directly until 2026-11-12, so a move fence that
/// read the write fence left it stopping at the margin and its test green.
///
/// **One exemption, DECFI's `cold.right`**: its algorithm compares the
/// column with the margin to scroll, `if (active_position.column ==
/// right_margin) scroll_left(left_margin, 1)` (`[[source: dec-std-070]]`
/// p. 5-38), which is not a stop, since its step goes on past the margin
/// to the line's end; and on a double line, with DECLRMM reset, the move
/// fence is the half's last column, so comparing with it would scroll the
/// region from a cursor DECFI moves along. Every other read is banned
/// there.
///
/// **The other horizontal moves read no right stop**: `HPR` and `HPB` pass
/// the margins, counting from the column a report names; `CHA`, `HPA` and
/// `CUP` address a column; `CBT`, `BS`, `CR`, `CNL` and `CPL` move left, to
/// a stop, the left margin or the line start; and `?45`'s walk ends on the
/// line above.
///
/// Positive controls: the probe finds `write_fence(` in `line_fence`,
/// `move_stop` in `forward_in_margins`, `cold.right` in
/// `move_fence_of_line` and `cold.fence` in `move_stop`.
#[test]
fn no_move_reads_the_write_fence() {
    let read = |f: &str| {
        let p = format!("{}/src/{f}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let (ops, rect, paint) = (read("ops.rs"), read("rect.rs"), read("paint.rs"));
    assert!(body_of(&paint, "line_fence").contains("write_fence("));
    assert!(body_of(&ops, "forward_in_margins").contains("move_stop"));
    assert!(body_of(&paint, "move_fence_of_line").contains("cold.right"));
    assert!(body_of(&paint, "move_stop").contains("cold.fence"));
    let every: Vec<&str> = NOT_IN_A_MOVE.to_vec();
    let decfi: Vec<&str> = every
        .iter()
        .copied()
        .filter(|b| *b != "cold.right")
        .collect();
    let (ops, rect) = (ops.as_str(), rect.as_str());
    let moves: [(&str, &str, &Vec<&str>); 12] = [
        (ops, "forward_in_margins", &every),
        (ops, "forward_to_line_end", &every),
        (ops, "forward_fenced", &every),
        (ops, "at_or_inside_right", &every),
        (ops, "back_in_margins", &every),
        (ops, "back_past_the_fence", &every),
        (ops, "back_fenced", &every),
        (ops, "tab_forward", &every),
        (ops, "tab_forward_fenced", &every),
        (ops, "tab_end", &every),
        (rect, "back_index", &every),
        (rect, "forward_index", &decfi),
    ];
    let mut found = Vec::new();
    for (src, name, banned) in moves {
        let body = body_of(src, name);
        for b in banned {
            if body.contains(b) {
                found.push(format!("{name}: {b}"));
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
