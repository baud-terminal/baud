//! **TAC, TALE, TATE and TCC** (`[[source: ecma48]]` 8.3.151 to 8.3.153,
//! 8.3.155): a stop "calling for" centring, leading edge or trailing edge
//! alignment, or the centring of a target character. After `HT` lands on
//! one, the string written is laid out as the stop says, a character at a
//! time. Every test clears the stops and sets one at position 9, column 8.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Row 0 as text, `.` for an empty position and `_` for a wide tail, and
/// the cursor's column.
fn laid(bytes: &str) -> (String, usize) {
    support::render(&WS, format!("\x1b[3g{bytes}").as_bytes(), |t| {
        let row = t.viewport().row(Row::ZERO)[..20]
            .iter()
            .map(|c| match c.content {
                x if x.is_wide_tail() => '_',
                x => x
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.'),
            })
            .collect();
        (row, t.cursor().1.index())
    })
}

/// **TAC centres the string on the stop**, the left edge of its cell, an
/// odd length's extra column after it (Jev's lean, ON_BOUNDARY).
#[test]
fn tac_centres_the_string_on_the_stop() {
    assert_eq!(laid("\x1b[9 b\tabc"), (".......abc..........".into(), 10));
    assert_eq!(laid("\x1b[9 b\tabcd"), ("......abcd..........".into(), 10));
}

/// **TALE ends the string at the stop**: "the (leading edge of the) last
/// graphic character of the string is placed at the tabulation stop", so
/// it ends in the cell before (Jev's lean, BEFORE_STOP).
#[test]
fn tale_ends_the_string_at_the_stop() {
    assert_eq!(laid("\x1b[9 a\tabc"), (".....abc............".into(), 8));
}

/// **TATE's string starts at the stop**, as an ordinary stop's.
#[test]
fn tate_starts_the_string_at_the_stop() {
    assert_eq!(laid("\x1b[9 `\tabc"), ("........abc.........".into(), 11));
}

/// **TCC puts the first target in the stop's cell** (Jev's lean,
/// TARGET_IN_CELL): `.` for 46; until it arrives the string starts at the
/// stop, and with none it stays there.
#[test]
fn tcc_puts_the_target_in_the_stops_cell() {
    assert_eq!(
        laid("\x1b[9;46 c\t12.5"),
        ("......12.5..........".into(), 10)
    );
    assert_eq!(
        laid("\x1b[9;46 c\t1234.5"),
        ("....1234.5..........".into(), 10)
    );
    assert_eq!(
        laid("\x1b[9;46 c\t123"),
        ("........123.........".into(), 11)
    );
    assert_eq!(
        laid("\x1b[9;46 c\t1.2.3"),
        (".......1.2.3........".into(), 12),
        "the first"
    );
}

/// **TCC's target defaults to 32**, the space, and may be a Latin-1
/// character, 160 to 255 (8.3.155).
#[test]
fn tcc_targets_the_space_by_default_and_latin1_letters() {
    assert_eq!(laid("\x1b[9 c\tab cd"), ("......ab cd.........".into(), 11));
    assert_eq!(
        laid("\x1b[9;233 c\t12\u{e9}5"),
        ("......12\u{e9}5..........".into(), 10)
    );
}

/// **SGR does not end the string; a cursor movement does** (Jev,
/// FORMAT_EFFECTOR, 0.92/0.92).
#[test]
fn sgr_keeps_the_string_and_a_movement_ends_it() {
    assert_eq!(
        laid("\x1b[9 a\tab\x1b[31mc"),
        (".....abc............".into(), 8)
    );
    assert_eq!(
        laid("\x1b[9 a\tab\x1b[Cc"),
        ("......ab.c..........".into(), 10)
    );
}

/// **A string that would move back past the line's start goes on as plain
/// text** (Jev's lean, AS_TEXT).
#[test]
fn a_string_at_the_line_start_goes_on_as_text() {
    assert_eq!(laid("\x1b[3 a\tabcd"), ("abcd................".into(), 4));
}

/// **`CHT` lands on it as `HT` does.**
#[test]
fn cht_lands_on_an_aligned_stop() {
    assert_eq!(
        laid("\x1b[9 a\x1b[Iabc"),
        (".....abc............".into(), 8)
    );
}

/// A wide character moves whole.
#[test]
fn a_wide_character_is_centred_whole() {
    assert_eq!(laid("\x1b[9 b\t中"), (".......中_...........".into(), 9));
}

/// **`HTS` sets an ordinary stop where an aligned one was**; `TSR` clears
/// one as any stop; `RIS` and DECST8C leave ordinary stops.
#[test]
fn hts_tsr_ris_and_decst8c_leave_ordinary_stops() {
    assert_eq!(
        laid("\x1b[9 a\x1b[1;9H\x1bH\x1b[1;1H\tabc").0,
        "........abc........."
    );
    assert_eq!(laid("\x1b[9 a\x1b[9 d\tabc").0, "...................a");
    assert_eq!(laid("\x1b[9 a\x1bc\tabc").0, "........abc.........");
    assert_eq!(laid("\x1b[9 a\x1b[?5W\tabc").0, "........abc.........");
}

/// `Pn` has no default value: an omitted one is counted, and so is a TCC
/// target 8.3.155 does not permit.
#[test]
fn a_missing_position_or_a_bad_target_is_counted() {
    support::render(&WS, b"\x1b[ b\x1b[9;10 c\x1b[9;300 c", |t| {
        assert_eq!(t.unsupported_param, 3);
    });
}

/// **DECTABSR reports an aligned stop as a stop**.
#[test]
fn dectabsr_reports_an_aligned_stop() {
    support::render(&WS, b"\x1b[3g\x1b[9 a\x1b[2$w", |t| {
        assert_eq!(t.reply(), b"\x1bP2$u9\x1b\\");
    });
}

/// **TCC's target is a code table position in the invoked code**
/// (8.3.155), before a set replaces it: under the UK set `#` is drawn `£`
/// and is still position 35, not 163; under DEC Special Graphics `q` is
/// drawn `─` and is position 113. A decoded `£` is its scalar value, 163.
#[test]
fn tcc_compares_the_code_table_position_received() {
    let aligned = "......12\u{a3}5..........";
    let trailing = "........12\u{a3}5........";
    assert_eq!(laid("\x1b(A\x1b[9;35 c\t12#5").0, aligned);
    assert_eq!(laid("\x1b(A\x1b[9;163 c\t12#5").0, trailing);
    assert_eq!(
        laid("\x1b(0\x1b[9;113 c\t12q5").0,
        "......12\u{2500}5.........."
    );
    assert_eq!(laid("\x1b[9;35 c\t12#5").0, "......12#5..........");
    assert_eq!(
        laid("\x1b(A\x1b[9;163 c\t1#\u{a3}5").0,
        "......1\u{a3}\u{a3}5..........",
        "the decoded one aligns, the replaced one before it does not"
    );
    assert_eq!(
        laid("\x1b*0\x1b[9;113 c\t12\x1bNq5").0,
        "......12\u{2500}5..........",
        "a single shift's code, G2 DEC Special Graphics"
    );
}

/// Every row after `bytes`, as [`laid`] draws row 0.
fn screen(bytes: &str) -> Vec<String> {
    support::render(&WS, format!("\x1b[3g{bytes}").as_bytes(), |t| {
        (0..4)
            .map(|y| {
                t.viewport().row(Row::from_index(y))[..20]
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|cp| char::from_u32(cp.value()))
                            .unwrap_or('.')
                    })
                    .collect()
            })
            .collect()
    })
}

/// **A format effector that scrolls ends the string** (Jev,
/// FORMAT_EFFECTOR): `LF`, `VT`, `FF` and `IND` at the bottom margin and
/// `RI` at the top leave the cursor on the row and column the string left
/// it, on the line that scrolled in, and what follows is plain text.
#[test]
fn a_format_effector_that_scrolls_ends_the_string() {
    for fe in ["\n", "\x0b", "\x0c", "\x1bD"] {
        let s = screen(&format!("\x1b[9 a\x1b[4;1H\tab{fe}c"));
        assert_eq!(s[2], "......ab............", "{fe:?}");
        assert_eq!(s[3], "........c...........", "{fe:?}");
    }
    let s = screen("\x1b[9 a\x1b[1;1H\tab\x1bMc");
    assert_eq!(s[0], "........c...........", "RI");
    assert_eq!(s[1], "......ab............", "RI");
}

/// **Within a scrolling region too**, where the lines move in place, and
/// on the last row below the region, where `LF` moves nothing.
#[test]
fn a_line_feed_in_or_below_a_region_ends_the_string() {
    let s = screen("\x1b[9 a\x1b[1;3r\x1b[3;1H\tab\nc");
    assert_eq!(s[1], "......ab............");
    assert_eq!(s[2], "........c...........");
    let s = screen("\x1b[9 a\x1b[2;3r\x1b[1;1H\tab\x1bMc");
    assert_eq!(s[0], "......abc...........", "RI above the region");
    let s = screen("\x1b[9 a\x1b[1;2r\x1b[4;1H\tab\nc");
    assert_eq!(s[3], "......abc...........");
}

/// **`CR` ends the string** where SLH's home is the column the string
/// left the cursor at, alone and folded with the `LF` after it.
#[test]
fn a_carriage_return_to_the_strings_column_ends_it() {
    let s = screen("\x1b[9 a\x1b[9 U\x1b[1;1H\tab\rc");
    assert_eq!(s[0], "......abc...........");
    let s = screen("\x1b[9 a\x1b[9 U\x1b[4;1H\tab\r\nc");
    assert_eq!(s[2], "......ab............");
    assert_eq!(s[3], "........c...........");
}

/// **An erase or edit of the string's line ends it** (Jev, FORMAT_EFFECTOR,
/// 0.92/0.92: "and an erase or edit of the line"; which ones, Jev's lean,
/// LINE_REACHED, 0.83/0.94): each of these leaves the cursor where the
/// string left it, column 8, and reaches row 0, so the `c` after it is
/// plain text there, where laid out it would end the string at column 7:
/// among them `ICH` and `DCH` from row 1 under SEE 0, whose shift crosses
/// into row 0, and `IL` and `DL`, which put the cursor on SLH's home at
/// the string's column, and `ICH` and `DCH` under HEM's PRECEDING.
#[test]
fn an_erase_or_edit_of_the_strings_line_ends_it() {
    let mut wrong = Vec::new();
    for edit in [
        "\x1b[K",
        "\x1b[1K",
        "\x1b[2K",
        "\x1b[X",
        "\x1b[P",
        "\x1b[@",
        "\x1b[J",
        "\x1b[1J",
        "\x1b[2J",
        "\x1b[?K",
        "\x1b[?J",
        "\x1b[N",
        "\x1b[O",
        "\x1b[S",
        "\x1b[T",
        "\x1b[ @",
        "\x1b[ A",
        "\x1b['}",
        "\x1b['~",
        "\x1b[1;1;1;20$z",
        "\x1b[1;1;1;20${",
        "\x1b[88;1;1;1;20$x",
        "\x1b[3;1;3;20;1;1;1;1$v",
        "\x1b[B\x1b[1J\x1b[A",
        "\x1b[Q\x1b[10h\x1b[B\x1b[@\x1b[A",
        "\x1b[Q\x1b[10h\x1b[B\x1b[P\x1b[A",
        "\x1b[9 U\x1b[L",
        "\x1b[9 U\x1b[M",
        "\x1b[0;1K",
        "\x1b[1;1J",
        "\x1b[?0;1K",
        "\x1b[?0;1J",
        "\x1b[10h\x1b[@",
        "\x1b[10h\x1b[P",
    ] {
        let (row, cursor) = laid(&format!("\x1b[9 a\tab{edit}c"));
        if row.find('c') != Some(8) || cursor != 9 {
            wrong.push(format!("{edit:?}: {row} cursor {cursor}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **One that reaches only other lines leaves it open**, and so does an
/// attribute change: `DECERA` and `DECFRA` on row 3, and `DECCARA` over
/// row 0. **So does one the margins make a no-op**, which erases and
/// shifts nothing: `IL` and `DL` above DECSTBM's region, and `ICH`, `DCH`,
/// `DECIC` and `DECDC` right of DECSLRM's, under no SEE, SEE's page and
/// HEM's PRECEDING.
#[test]
fn an_erase_of_another_line_or_an_attribute_change_keeps_it() {
    for edit in ["\x1b[4;1;4;20$z", "\x1b[88;4;1;4;20$x", "\x1b[1;1;1;20;1$r"] {
        let (row, _) = laid(&format!("\x1b[9 a\tab{edit}c"));
        assert_eq!(row.find('c'), Some(7), "{edit:?}: {row}");
    }
    let region = "\x1b[2;4r\x1b[H";
    let margins = "\x1b[?69h\x1b[1;5s\x1b[1;7H";
    let mut wrong = Vec::new();
    for (pre, edit) in [
        (region, "\x1b[L"),
        (region, "\x1b[M"),
        (margins, "\x1b[@"),
        (margins, "\x1b[P"),
        (margins, "\x1b['}"),
        (margins, "\x1b['~"),
        (margins, "\x1b[Q\x1b[@"),
        (margins, "\x1b[Q\x1b[P"),
        (margins, "\x1b[10h\x1b[@"),
        (margins, "\x1b[10h\x1b[P"),
    ] {
        let (row, _) = laid(&format!("{pre}\x1b[9 a\tab{edit}c"));
        if row.find('c') != Some(7) {
            wrong.push(format!("{pre:?} {edit:?}: {row}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
