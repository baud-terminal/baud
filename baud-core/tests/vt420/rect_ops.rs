//! **The VT420 rectangular area operations**: DECCRA, DECFRA, DECERA,
//! DECSERA, DECCARA, DECRARA, DECSACE and DECRQCRA, each from
//! `[[source: dec-std-070]]` § 5.12.1 and § 5.12.2 (pp. 5-168 to 5-181),
//! with `[[source: vt420-rm]]` chapter 9 beside it.
//!
//! What every one shares, from each one's own text: "The coordinates of the
//! rectangular area are affected by the setting of Origin Mode. This control
//! is not otherwise affected by the margins"; "If a value exceeds the width or
//! height of the active page, it is treated as the width or height of the
//! active page"; "Pt must be less or equal to Pb, and Pl must be less than or
//! equal to Pr, otherwise the entire control function is ignored"; and the
//! active position does not move.
//!
//! The screen is 20 by 8, each row filled with a letter of its own, so a
//! cell that moved shows another row's letter.

use crate::support;
use baud_core::*;
use baud_domain::{ImageBudget, ImagePx};

const ROWS: usize = 8;
const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// Every row filled with its letter, `a` to `h`, and the cursor parked at
/// row 8, column 20 by the last write.
fn fill() -> String {
    let mut s = String::new();
    for (r, letter) in "abcdefgh".chars().enumerate() {
        s.push_str(&format!("\x1b[{};1H", r + 1));
        s.extend(std::iter::repeat_n(letter, COLS));
    }
    s
}

/// One cell as a character: `.` for an unoccupied cell, `]` for a wide
/// tail.
fn ch(c: PackedCell) -> char {
    if c.content.is_empty() {
        return '.';
    }
    if c.content.is_wide_tail() {
        return ']';
    }
    c.content
        .as_codepoint()
        .and_then(|cp| char::from_u32(cp.value()))
        .unwrap_or('?')
}

fn rows_of(t: &Term<'_>) -> Vec<String> {
    (0..ROWS)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row")[..COLS]
                .iter()
                .map(|&c| ch(c))
                .collect()
        })
        .collect()
}

fn cell(t: &Term<'_>, row: usize, col: usize) -> PackedCell {
    t.row(Row::from_index(row - 1)).expect("a row")[col - 1]
}

fn attrs(t: &Term<'_>, row: usize, col: usize) -> Attrs {
    t.style_of(cell(t, row, col)).attrs()
}

/// Feed `bytes` to a fresh 20 by 8 terminal and read it with `f`.
fn after<R>(bytes: &str, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    support::render(&WS, bytes.as_bytes(), f)
}

// ---- DECCRA ------------------------------------------------------------

/// "the terminal copies the character values and attributes in the
/// specified rectangular area ... to the area specified by the coordinate
/// Ptd, Pld ... The Active Position does not change" (p. 5-169).
#[test]
fn deccra_copies_the_rectangle_and_its_renditions_and_leaves_the_cursor() {
    // Row 2's columns 2-4 bold, then the cursor parked at 1;1.
    let bytes = format!(
        "{}\x1b[2;2H\x1b[1mbbb\x1b[m\x1b[1;1H\x1b[2;2;3;4;1;6;10;1$v",
        fill()
    );
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            s[5], "fffffffffbbbffffffff",
            "row 2's columns 2-4 at row 6, column 10"
        );
        assert_eq!(s[6], "gggggggggcccgggggggg", "row 3's at row 7");
        assert_eq!(s[1], "bbbbbbbbbbbbbbbbbbbb", "the source stays");
        assert!(attrs(t, 6, 10).has(Attrs::BOLD), "the rendition is copied");
        assert!(
            !attrs(t, 6, 13).has(Attrs::BOLD),
            "and only inside the copy"
        );
        assert_eq!(t.cursor(), (Row::from_index(0), Col::at_index(0)));
        assert_eq!(t.unsupported, 0);
    });
}

/// **A copy onto itself is a snapshot**: the source is read whole before
/// the destination is written, so an overlapping copy does not smear. Down
/// one row and right one column, then up and left.
#[test]
fn deccra_copies_an_overlapping_rectangle_as_it_was() {
    let mut letters = String::new();
    for r in 1..=4 {
        letters.push_str(&format!("\x1b[{r};1H0123456789"));
    }
    let down = format!("{letters}\x1b[1;1;3;5;1;2;2;1$v");
    after(&down, |t| {
        let s = rows_of(t);
        assert_eq!(&s[0][..10], "0123456789");
        assert_eq!(&s[1][..10], "0012346789", "row 1's 0-4 at row 2, column 2");
        assert_eq!(&s[2][..10], "0012346789");
        assert_eq!(&s[3][..10], "0012346789");
    });
    let up = format!("{letters}\x1b[2;2;4;6;1;1;1;1$v");
    after(&up, |t| {
        let s = rows_of(t);
        assert_eq!(&s[0][..10], "1234556789", "row 2's 1-5 at row 1, column 1");
        assert_eq!(&s[2][..10], "1234556789");
        assert_eq!(
            &s[3][..10],
            "0123456789",
            "the last source row is not a destination"
        );
    });
}

/// "If the destination rectangle specified is partially off of the Page,
/// clipping of information will occur" -- and a destination past the page
/// is "treated as the width or height of the active page".
#[test]
fn deccra_clips_at_the_page_and_clamps_a_destination_past_it() {
    let bytes = format!("{}\x1b[1;1;2;5;1;8;18;1$v", fill());
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            s[7], "hhhhhhhhhhhhhhhhhaaa",
            "row 1's first three columns, the rest clipped"
        );
    });
    let past = format!("{}\x1b[1;1;1;3;1;99;99;1$v", fill());
    after(&past, |t| {
        assert_eq!(
            rows_of(t)[7],
            "hhhhhhhhhhhhhhhhhhha",
            "the corner, one column left"
        );
    });
}

/// Under DECOM the corners count from the margins' origin, and **only** the
/// origin moves: the copy reaches past the bottom margin, "not otherwise
/// affected by the margins". xterm clamps into the margins instead.
#[test]
fn deccra_under_decom_counts_from_the_origin_and_passes_the_margins() {
    let bytes = format!("{}\x1b[2;4r\x1b[?6h\x1b[1;1;1;3;1;5;1;1$v", fill());
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            s[5], "bbbfffffffffffffffff",
            "origin row 2: Pts 1 is row 2, Ptd 5 is row 6"
        );
    });
}

/// Inverted corners are "otherwise the entire control function is ignored",
/// for every operation, and nothing is counted: it is the specified answer.
#[test]
fn inverted_corners_are_ignored_by_every_operation() {
    for op in [
        "\x1b[3;1;2;5;1;6;6;1$v",
        "\x1b[42;3;1;2;5$x",
        "\x1b[3;1;2;5$z",
        "\x1b[3;1;2;5${",
        "\x1b[3;1;2;5;7$r",
        "\x1b[3;1;2;5;7$t",
        "\x1b[1;5;3;2$z",
    ] {
        let bytes = format!("{}{op}", fill());
        after(&bytes, |t| {
            let want = after(&fill(), |u| rows_of(u));
            assert_eq!(rows_of(t), want, "{op:?} changed the screen");
            assert!(!attrs(t, 2, 2).has(Attrs::REVERSE), "{op:?}");
            assert_eq!(t.unsupported, 0, "{op:?}");
        });
    }
}

// ---- DECFRA ------------------------------------------------------------

/// "replacing both the character and the rendition present in those
/// character positions with the character defined by the decimal value of
/// Pch and the current renditions set through the SGR command" (p. 5-170).
#[test]
fn decfra_fills_with_the_character_in_the_current_rendition() {
    let bytes = format!("{}\x1b[1;31m\x1b[42;2;3;3;6$x\x1b[m", fill());
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(s[1], "bb****bbbbbbbbbbbbbb");
        assert_eq!(s[2], "cc****cccccccccccccc");
        let w = t.style_of(cell(t, 2, 3));
        assert!(w.attrs().has(Attrs::BOLD));
        assert_eq!(w.fg(), ColorWord::indexed(Chan::from_u8(1)));
        assert_eq!(t.cursor(), (Row::from_index(7), Col::at_index(19)));
    });
}

/// "The decimal value refers to the character in the current GL and GR
/// 'in-use' character table": GL through DEC Special Graphics after
/// `ESC ( 0`, which marks the cells as that set's; GR the Latin-1 supplement.
#[test]
fn decfra_reads_pch_through_gl_and_gr() {
    let bytes = format!("{}\x1b(0\x1b[113;1;1;1;2$x\x1b(B\x1b[233;2;1;2;2$x", fill());
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            s[0].chars().take(3).collect::<String>(),
            "──a",
            "113 is `q`, a horizontal line in DEC Special Graphics"
        );
        assert!(cell(t, 1, 1).flags.is_graphics());
        assert_eq!(
            s[1].chars().take(3).collect::<String>(),
            "ééb",
            "233 is U+00E9"
        );
        assert!(!cell(t, 2, 1).flags.is_graphics());
    });
}

/// Pch omitted or 0 is the default, 32; outside 32-126 and 160-255 "the
/// command is ignored", which is counted.
#[test]
fn decfra_defaults_to_a_space_and_ignores_pch_outside_its_ranges() {
    after(&format!("{}\x1b[;1;1;1;3$x", fill()), |t| {
        assert_eq!(&rows_of(t)[0][..4], "   a");
    });
    after(&format!("{}\x1b[$x", fill()), |t| {
        assert!(
            rows_of(t).iter().all(|r| r == &" ".repeat(COLS)),
            "a bare DECFRA fills the page"
        );
    });
    for pch in [127, 128, 159, 256, 9] {
        after(&format!("{}\x1b[{pch};1;1;1;3$x", fill()), |t| {
            assert_eq!(&rows_of(t)[0][..4], "aaaa", "Pch {pch}");
            assert_eq!(t.unsupported_param, 1, "Pch {pch} is counted");
        });
    }
}

// ---- DECERA and DECSERA ------------------------------------------------

/// "all characters are replaced with the Space character (2/0). Any
/// unoccupied character positions remain unoccupied", "clearing all
/// character attributes" (p. 5-171).
#[test]
fn decera_spaces_what_is_written_and_leaves_the_unoccupied_unoccupied() {
    // Row 1: `xy` bold red and protected, then nothing.
    let bytes = "\x1b[1;31m\x1b[1\"qxy\x1b[0\"q\x1b[m\x1b[44m\x1b[1;1;2;5$z";
    after(bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            &s[0][..6],
            "  ....",
            "the written cells are spaces, the rest still empty"
        );
        for c in 1..=5 {
            let x = cell(t, 1, c);
            assert_eq!(
                t.style_of(x),
                StyleWord::DEFAULT,
                "column {c}: the default rendition"
            );
            assert!(!x.is_protected(), "column {c}: no protection");
        }
    });
}

/// "This control function erases unprotected characters ... Unoccupied
/// character positions are not affected. This sequence does not change or
/// clear any video character attributes (SGR or DECSCA)" (p. 5-172).
#[test]
fn decsera_spares_protected_and_unoccupied_cells_and_keeps_renditions() {
    let bytes = "\x1b[4mab\x1b[1\"qcd\x1b[0\"qef\x1b[m\x1b[1;1;1;8${";
    after(bytes, |t| {
        let s = rows_of(t);
        assert_eq!(
            &s[0][..8],
            "  cd  ..",
            "c and d are protected; 7 and 8 were never written"
        );
        assert!(attrs(t, 1, 1).has(Attrs::UNDERLINE), "the rendition stays");
        assert!(cell(t, 1, 3).is_protected());
        assert!(cell(t, 1, 7).content.is_empty());
    });
}

/// DECSERA's space is DECSED's: an ECMA-48 guard on the cell stays (Jev,
/// AGREE, 1.00 and 0.98, 2026-10-19) and so does an `OSC 8` link (Jev's
/// lean, KEEP); the line-drawing flag goes with the character.
#[test]
fn decsera_keeps_the_guard_and_the_link() {
    after("\x1bVab\x1bW\x1b[1;1;1;2${", |t| {
        assert_eq!(&rows_of(t)[0][..2], "  ");
        assert!(cell(t, 1, 1).flags.is_guarded() && cell(t, 1, 2).flags.is_guarded());
    });
    after(
        "\x1b]8;;http://x/\x1b\\ab\x1b]8;;\x1b\\\x1b[1;1;1;2${",
        |t| {
            assert_eq!(&rows_of(t)[0][..2], "  ");
            assert_eq!(
                t.link_uri(cell(t, 1, 1).flags.link()),
                Some(&b"http://x/"[..])
            );
        },
    );
    after("\x1b(0qq\x1b(B\x1b[1;1;1;2${", |t| {
        assert!(!cell(t, 1, 1).flags.is_graphics());
    });
}

// ---- DECCARA, DECRARA and DECSACE --------------------------------------

/// "This sequence changes the video attributes (bold, blink, underline, and
/// reverse) ... without altering any characters" -- and the colours stay.
#[test]
fn deccara_changes_the_four_aspects_and_nothing_else() {
    let bytes = format!(
        "{}\x1b[2;2H\x1b[3;32mbbbb\x1b[m\x1b[*x\x1b[2*x\x1b[2;2;2;3;1;4;7$r",
        fill()
    );
    after(&bytes, |t| {
        let w = t.style_of(cell(t, 2, 2));
        let a = w.attrs();
        assert!(a.has(Attrs::BOLD) && a.has(Attrs::UNDERLINE) && a.has(Attrs::REVERSE));
        assert!(a.has(Attrs::ITALIC), "italic is not one of the four");
        assert_eq!(
            w.fg(),
            ColorWord::indexed(Chan::from_u8(2)),
            "the colour stays"
        );
        assert!(!attrs(t, 2, 4).has(Attrs::BOLD), "column 4 is outside");
        assert_eq!(
            rows_of(t)[1],
            "bbbbbbbbbbbbbbbbbbbb",
            "no character changed"
        );
    });
}

/// "When multiple parameters are used, they are cumulative": `0` then `5`
/// clears and blinks; and with no attribute parameter at all the default is
/// 0, "which will clear all video attributes in the specified area". Values
/// the text does not list are ignored, `8` among them (xterm's extension).
#[test]
fn deccara_is_cumulative_defaults_to_0_and_ignores_unlisted_values() {
    let set = format!("{}\x1b[2*x\x1b[1;1;1;3;1;4$r", fill());
    after(&format!("{set}\x1b[1;1;1;3;0;5$r"), |t| {
        assert_eq!(attrs(t, 1, 1), Attrs::BLINK);
    });
    after(&format!("{set}\x1b[1;1;1;3$r"), |t| {
        assert_eq!(attrs(t, 1, 1), Attrs::NONE);
    });
    after(&format!("{set}\x1b[1;1;1;3;8;3$r"), |t| {
        assert_eq!(
            attrs(t, 1, 1),
            Attrs::NONE,
            "8 and 3 are not valid, so 0 applies"
        );
    });
    after(&format!("{}\x1b[2*x\x1b[1;1;1;3;8$r", fill()), |t| {
        assert!(!attrs(t, 1, 1).has(Attrs::HIDDEN));
    });
}

/// DECRARA reverses: "set the specified attribute bits if it was not
/// previously set, and reset the bit(s) if it was already set" -- and with
/// no valid attribute parameter, "the command is ignored" (DEC STD 070
/// p. 5-176), not "0 Reverse all attributes (default)" as the VT420 and
/// VT510 manuals have it.
#[test]
fn decrara_reverses_and_ignores_a_request_with_no_valid_attribute() {
    let base = format!("{}\x1b[2*x\x1b[1;1;1;2;1$r", fill());
    after(&format!("{base}\x1b[1;1;1;3;1;7$t"), |t| {
        assert_eq!(
            attrs(t, 1, 1),
            Attrs::REVERSE,
            "bold reversed off, reverse on"
        );
        assert_eq!(attrs(t, 1, 3), Attrs::BOLD.with(Attrs::REVERSE));
    });
    after(&format!("{base}\x1b[1;1;1;3$t"), |t| {
        assert_eq!(
            attrs(t, 1, 1),
            Attrs::BOLD,
            "no attribute parameter: ignored"
        );
        assert_eq!(attrs(t, 1, 3), Attrs::NONE);
    });
    after("\x1b[2*x\x1b[1;1;1;3$t", |t| {
        assert!(
            cell(t, 1, 1).content.is_empty(),
            "ignored: no blank made of an empty cell"
        );
    });
    after(&format!("{base}\x1b[1;1;1;3;0$t"), |t| {
        let all = Attrs::UNDERLINE.with(Attrs::BLINK).with(Attrs::REVERSE);
        assert_eq!(attrs(t, 1, 1), all, "0 reverses all four");
    });
}

/// DECSACE: "When Ps = 0 or 1, DECCARA and DECRARA affects the stream of
/// character positions beginning with the first character position
/// specified in the command, and ending with the second ... These
/// unoccupied positions are not affected"; with 2, every position in the
/// rectangle, "these character positions are changed to blanks and are
/// affected" (p. 5-177). The stream is the default.
#[test]
fn decsace_chooses_the_stream_or_the_rectangle() {
    // Rows 1-3 written to column 10 only.
    let mut bytes = String::new();
    for r in 1..=3 {
        bytes.push_str(&format!("\x1b[{r};1Hxxxxxxxxxx"));
    }
    let stream = format!("{bytes}\x1b[1;3;3;5;7$r");
    after(&stream, |t| {
        assert!(attrs(t, 1, 3).has(Attrs::REVERSE) && attrs(t, 1, 10).has(Attrs::REVERSE));
        assert!(
            !attrs(t, 1, 2).has(Attrs::REVERSE),
            "the stream starts at the first corner"
        );
        assert!(
            attrs(t, 2, 1).has(Attrs::REVERSE),
            "and runs through whole rows"
        );
        assert!(attrs(t, 3, 5).has(Attrs::REVERSE) && !attrs(t, 3, 6).has(Attrs::REVERSE));
        assert!(
            cell(t, 1, 15).content.is_empty(),
            "an unoccupied position is not affected"
        );
        assert_eq!(t.style_of(cell(t, 1, 15)), StyleWord::DEFAULT);
    });
    let rect = format!("{bytes}\x1b[2*x\x1b[1;3;3;12;7$r");
    after(&rect, |t| {
        assert!(!attrs(t, 2, 1).has(Attrs::REVERSE), "only the rectangle");
        assert!(attrs(t, 2, 3).has(Attrs::REVERSE));
        assert_eq!(
            ch(cell(t, 1, 12)),
            ' ',
            "an unoccupied position becomes a blank"
        );
        assert!(attrs(t, 1, 12).has(Attrs::REVERSE), "and is affected");
        assert!(
            cell(t, 1, 13).content.is_empty(),
            "past the rectangle it is left alone"
        );
    });
}

/// DECRQSS reports DECSACE (`[[source: vt420-rm]]` Table 12-4, "*x"); `RIS`
/// returns it to the stream (STD 070's host_state_init, 070-4 p. 15) and `DECSTR`, whose
/// table does not name it, keeps it.
#[test]
fn decsace_is_reported_kept_by_decstr_and_reset_by_ris() {
    let ask = "\x1bP$q*x\x1b\\";
    after(ask, |t| assert_eq!(t.reply(), b"\x1bP1$r0*x\x1b\\"));
    after(&format!("\x1b[2*x{ask}"), |t| {
        assert_eq!(t.reply(), b"\x1bP1$r2*x\x1b\\")
    });
    after(&format!("\x1b[2*x\x1b[!p{ask}"), |t| {
        assert_eq!(t.reply(), b"\x1bP1$r2*x\x1b\\", "DECSTR keeps it")
    });
    after(&format!("\x1b[2*x\x1bc{ask}"), |t| {
        assert_eq!(t.reply(), b"\x1bP1$r0*x\x1b\\", "RIS resets it")
    });
    after("\x1b[3*x", |t| {
        assert_eq!(t.unsupported_param, 1, "Ps 3 is counted")
    });
}

// ---- DECRQCRA ----------------------------------------------------------

/// vttest's expectation, `CHK(n) = (-(n)) & 0xffff`.
fn chk(n: u32) -> String {
    format!("{:04X}", n.wrapping_neg() & 0xffff)
}

/// "a checksum of the indicated rectangular area of the indicated page will
/// be generated and reported to the host in a Checksum Report (DECCKSR)",
/// `DCS Pid ! ~ D...D ST`, four capital hex digits (pp. 5-179, 5-181). One
/// cell is its code; bold adds 0x80, as vttest counts it.
#[test]
fn decrqcra_reports_the_checksum_of_a_cell_and_of_the_page() {
    after("AB\x1b[1mC\x1b[m\x1b[7;1;1;1;1;1*y", |t| {
        assert_eq!(t.reply(), format!("\x1bP7!~{}\x1b\\", chk(0x41)).as_bytes());
    });
    after("AB\x1b[1mC\x1b[m\x1b[1;1;1;3;1;3*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(0x43 + 0x80)).as_bytes()
        );
    });
    // Pp 0: all page memory, the rectangle ignored. Unwritten cells count
    // nothing.
    after("AB\x1b[1mC\x1b[m\x1b[2;0;5;5;5;5*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP2!~{}\x1b\\", chk(0x41 + 0x42 + 0x43 + 0x80)).as_bytes()
        );
    });
    // Eight-bit controls after S8C1T.
    after("A\x1b G\x1b[1;1;1;1;1;1*y", |t| {
        let want = [b"\x90".as_slice(), b"1!~", chk(0x41).as_bytes(), b"\x9c"].concat();
        assert_eq!(t.reply(), want.as_slice());
    });
}

/// **A written space counts**, as xterm's DEC mode counts every drawn cell
/// and vttest's own expected sums count the spaces between words: its
/// title-row sum runs to the last non-space cell, and its whole-page sum
/// adds every cell it wrote. An unwritten cell counts nothing.
#[test]
fn decrqcra_counts_a_written_space_and_not_an_unwritten_cell() {
    after("A B\x1b[1;1;1;1;1;3*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(0x41 + 0x20 + 0x42)).as_bytes()
        );
    });
    after("A B\x1b[3;1HC  D \x1b[1;1*y", |t| {
        assert_eq!(
            t.reply(),
            format!(
                "\x1bP1!~{}\x1b\\",
                chk(0x41 + 0x20 + 0x42 + 0x43 + 0x20 + 0x20 + 0x44 + 0x20)
            )
            .as_bytes()
        );
    });
}

/// "Pt must be less than or equal to Pb, and Pl must be less than or equal
/// to Pr or the sequence is ignored": no reply.
#[test]
fn decrqcra_with_inverted_corners_is_not_answered() {
    after("AB\x1b[1;1;3;1;1;1*y", |t| {
        assert_eq!(t.reply(), b"");
        assert_eq!(
            t.unsupported, 0,
            "recognised, and answered as the text says: not at all"
        );
    });
}

/// **Under DECOM a request with a page and no rectangle sums the page**:
/// "If the final four parameters are unspecified, a checksum of page Pp is
/// generated" (`[[source: dec-std-070]]` p. 5-179), and the page is the
/// whole page, not the margins' share of it -- the same answer `Pp` 0 gets.
#[test]
fn decrqcra_of_a_page_under_decom_sums_the_whole_page() {
    let set = "A\x1b[8;1HB\x1b[3;6r\x1b[?6h";
    let page = after(&format!("{set}\x1b[1;0*y"), |t| t.reply().to_vec());
    assert_eq!(
        page,
        format!("\x1bP1!~{}\x1b\\", chk(0x41 + 0x42)).as_bytes()
    );
    after(&format!("{set}\x1b[1;1*y"), |t| {
        assert_eq!(
            t.reply(),
            page.as_slice(),
            "Pp 1, the rectangle unspecified"
        );
    });
    after(&format!("{set}\x1b[1;1;;;;*y"), |t| {
        assert_eq!(t.reply(), page.as_slice(), "the four given empty");
    });
    // The controls: one corner given, so the rectangle counts from the
    // margins' origin, row 3, and row 1's A is outside it -- the first
    // corner, or the last parameter alone.
    after(&format!("{set}\x1b[1;1;1*y"), |t| {
        assert_eq!(t.reply(), format!("\x1bP1!~{}\x1b\\", chk(0x42)).as_bytes());
    });
    after(&format!("{set}\x1b[1;1;;;;20*y"), |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(0x42)).as_bytes(),
            "Pr alone"
        );
    });
}

/// **A cell written under a national replacement set counts the code it
/// was received as**: DEC STD 070's display stores "code = parv[0] & 0x7F"
/// and the set beside it (p. 5-140), and xterm's checksum sums that code
/// (`charSeen` through `xtermCharSetDec`, whose national arms keep it,
/// `[[source: xterm-charsets-c]]`). German's `[` is `Ä` and counts 0x5B;
/// Norwegian/Danish draws the same `Ä` from `@` and counts 0x40; an `Ä`
/// sent as UTF-8 under ASCII counts its own value, 0xC4.
#[test]
fn decrqcra_counts_a_national_character_as_the_code_received() {
    after("\x1b(K[\x1b(B\x1b[1;1;1;1;1;1*y", |t| {
        assert_eq!(
            cell(t, 1, 1).content.as_codepoint().map(|c| c.value()),
            Some(0xC4)
        );
        assert_eq!(t.reply(), format!("\x1bP1!~{}\x1b\\", chk(0x5B)).as_bytes());
    });
    after("\x1b(E@\x1b(B\x1b[1;1;1;1;1;1*y", |t| {
        assert_eq!(
            cell(t, 1, 1).content.as_codepoint().map(|c| c.value()),
            Some(0xC4)
        );
        assert_eq!(t.reply(), format!("\x1bP1!~{}\x1b\\", chk(0x40)).as_bytes());
    });
    after("\u{C4}\x1b[1;1;1;1;1;1*y", |t| {
        assert_eq!(t.reply(), format!("\x1bP1!~{}\x1b\\", chk(0xC4)).as_bytes());
    });
    // A long run takes the bulk path, a short one the per-character path.
    after("\x1b(K[[[[[[[[[[[[[[[[\x1b[1;1;1;1;1;16*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(16 * 0x5B)).as_bytes()
        );
    });
    // DECFRA's Pch "refers to the character in the current GL ... table"
    // (p. 5-170): the code is Pch.
    after("\x1b(K\x1b[91;1;1;1;2$x\x1b[1;1;1;1;1;2*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(2 * 0x5B)).as_bytes()
        );
    });
}

/// **A character sent as UTF-8 counts its own value whatever set GL
/// holds**: it was not received as a GL code, and p. 5-140 stores a code
/// above 126 with the GR set, never with GL. So a UTF-8 `Ä` under German
/// counts 0xC4, as it does under ASCII, beside a GL `[` that German draws
/// as the same `Ä` and that counts 0x5B. REP copies each as it was
/// written, under the same set. A wide character carries no tag either,
/// which its sum cannot show, so the cells say it.
#[test]
fn decrqcra_counts_a_utf8_character_as_itself_under_a_national_set() {
    for (ch, code) in [('\u{C4}', 0xC4), ('\u{E4}', 0xE4), ('\u{DC}', 0xDC)] {
        after(&format!("\x1b(K{ch}\x1b(B\x1b[1;1;1;1;1;1*y"), |t| {
            assert_eq!(
                t.reply(),
                format!("\x1bP1!~{}\x1b\\", chk(code)).as_bytes(),
                "U+{code:04X}"
            );
        });
    }
    // A run long enough for the bulk writer.
    let run = "\u{C4}".repeat(16);
    after(&format!("\x1b(K{run}\x1b(B\x1b[1;1;1;1;1;16*y"), |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(16 * 0xC4)).as_bytes()
        );
    });
    after("\x1b(K[\u{C4}\x1b(B\x1b[1;1;1;1;1;2*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(0x5B + 0xC4)).as_bytes(),
            "a GL `[` and a UTF-8 `Ä`, side by side"
        );
    });
    after("\x1b(K\u{C4}\x1b[2b\x1b(B\x1b[1;1;1;1;1;3*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(3 * 0xC4)).as_bytes(),
            "REP after a UTF-8 `Ä`"
        );
    });
    after("\x1b(K[\x1b[2b\x1b(B\x1b[1;1;1;1;1;3*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(3 * 0x5B)).as_bytes(),
            "REP after a GL `[`"
        );
    });
    let cjk = "\u{4E2D}".repeat(8);
    for text in ["\u{4E2D}", cjk.as_str()] {
        after(&format!("\x1b(K{text}"), |t| {
            for col in (1..=2 * text.chars().count()).step_by(2) {
                assert_eq!(cell(t, 1, col).flags.national(), None, "column {col}");
            }
        });
    }
}

/// **An erased position the text leaves unoccupied counts nothing**: DECERA
/// "Any unoccupied character positions remain unoccupied" (p. 5-171),
/// DECSERA "Unoccupied character positions are not affected" (p. 5-172),
/// and ED, EL and ECH store the empty character (pp. 5-153 to 5-157). An
/// occupied position DECERA spaces counts as a space. xterm marks every
/// erased cell drawn and counts each (`ScrnFillRectangle`,
/// `ScrnWipeRectangle`, `ClearInLine`), a departure from those texts.
#[test]
fn decrqcra_leaves_out_what_an_erase_leaves_unoccupied() {
    after("AB\x1b[1;1;2;20$z\x1b[1;1;1;1;2;20*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(2 * 0x20)).as_bytes()
        );
    });
    after("AB\x1b[1;1;2;20${\x1b[1;1;1;1;2;20*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(2 * 0x20)).as_bytes()
        );
    });
    after(
        "ABCD\x1b[1;2H\x1b[1K\x1b[1;4H\x1b[X\x1b[2;1H\x1b[J\x1b[1;1;1;1;2;20*y",
        |t| {
            assert_eq!(t.reply(), format!("\x1bP1!~{}\x1b\\", chk(0x43)).as_bytes());
        },
    );
}

/// **DECSED and DECSEL write a space and keep the rendition**: "changed to
/// the Space character (2/0) ... The character rendition and attributes
/// associated with each position do not change ... empty character
/// positions, are not affected" (pp. 5-159, 5-162). The space counts with
/// its rendition's weight, bold 0x80 here; the empty position nothing.
#[test]
fn decrqcra_counts_what_decsed_spaced_with_its_rendition() {
    after("\x1b[1mAB\x1b[m\x1b[?2K\x1b[1;1;1;1;1;20*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(2 * (0x20 + 0x80))).as_bytes()
        );
    });
    after("\x1b[1mAB\x1b[m\x1b[?2J\x1b[1;1;1;1;1;20*y", |t| {
        assert_eq!(
            t.reply(),
            format!("\x1bP1!~{}\x1b\\", chk(2 * (0x20 + 0x80))).as_bytes()
        );
    });
}

// ---- wide characters at an edge ----------------------------------------

/// **A wide character an edge cuts goes whole**: the half outside becomes a
/// written space in its own rendition (Jev 2026-10-18, ORPHAN_SPACE), for
/// DECERA, DECSERA, DECFRA and the destination of DECCRA alike.
#[test]
fn an_edit_that_cuts_a_wide_character_spaces_its_other_half() {
    // U+4E2D at columns 1-2 and 3-4, bold.
    let wide = "\x1b[1m\u{4e2d}\u{4e2d}\x1b[m";
    for op in [
        "\x1b[1;2;1;3$z",
        "\x1b[1;2;1;3${",
        "\x1b[42;1;2;1;3$x",
        "\x1b[1;7;1;8;1;1;2;1$v",
    ] {
        after(&format!("{wide}{op}"), |t| {
            let s = rows_of(t);
            assert_eq!(ch(cell(t, 1, 1)), ' ', "{op:?}: the lead outside");
            assert_eq!(ch(cell(t, 1, 4)), ' ', "{op:?}: the tail outside");
            assert!(
                attrs(t, 1, 1).has(Attrs::BOLD),
                "{op:?}: in its own rendition, {s:?}"
            );
        });
    }
    // DECSERA spares a protected character whole.
    after("\x1b[1\"q\u{4e2d}\u{4e2d}\x1b[0\"q\x1b[1;2;1;3${", |t| {
        assert_eq!(rows_of(t)[0].chars().take(4).collect::<String>(), "中]中]");
    });
}

/// **DECCARA changes only the cells inside**, a cut character's included
/// (Jev 2026-10-18, INSIDE_ONLY): the positions are the unit.
#[test]
fn deccara_on_a_cut_wide_character_changes_only_the_cell_inside() {
    after("\u{4e2d}\x1b[2*x\x1b[1;2;1;2;1$r", |t| {
        assert!(!attrs(t, 1, 1).has(Attrs::BOLD));
        assert!(attrs(t, 1, 2).has(Attrs::BOLD));
        assert_eq!(rows_of(t)[0].chars().take(2).collect::<String>(), "中]");
    });
}

/// **The half a cut leaves keeps DECSCA's protection and nothing else of
/// the character's flags**: its ECMA-48 guard and its `OSC 8` link go
/// (Jev's lean, PROTECTION_ONLY, 0.67/0.70 and 0.83/0.77 on a re-ask, log
/// 874-877; STD 070 has no character two columns wide). xterm's
/// `Clear1Cell` changes the character alone and keeps every attribute.
#[test]
fn the_half_a_cut_leaves_keeps_its_protection_alone() {
    let bytes =
        "\x1b[1\"q\x1bV\x1b]8;;http://example.invalid/\x1b\\\u{4e2d}\x1b]8;;\x1b\\\x1bW\x1b[0\"q";
    after(&format!("{bytes}\x1b[1;2;1;2$z"), |t| {
        let half = cell(t, 1, 1);
        assert_eq!(ch(half), ' ');
        assert!(half.is_protected(), "the protection stays");
        assert!(!half.flags.is_guarded(), "the guard goes");
        assert_eq!(half.flags.link(), LinkId::NONE, "the link goes");
    });
    // The control: before the cut the character carries all three.
    after(bytes, |t| {
        let lead = cell(t, 1, 1);
        assert!(lead.is_protected() && lead.flags.is_guarded());
        assert_ne!(lead.flags.link(), LinkId::NONE);
    });
}

/// **DECFRA fills one character a position under the wide ambiguous-width
/// policy too**: "the terminal fills the rectangular area of character
/// positions ... with the character defined by the decimal value of Pch"
/// (`[[source: dec-std-070]]` p. 5-170, `[[source: vt510-decfra]]`), and a
/// DEC position holds one character. `Pch` 176, U+00B0, is East Asian
/// Ambiguous: written as text under the policy it takes two columns, and
/// filled it takes each of four.
#[test]
fn decfra_fills_a_position_each_under_wide_ambiguous_width() {
    support::with_term(&WS, |t| {
        t.set_ambiguous_width(AmbiguousWidth::Wide);
        support::feed(t, b"\x1b[176;1;1;1;4$x\x1b[2;1H\xc2\xb0");
        let s = rows_of(t);
        assert_eq!(&s[0][..4 * '\u{b0}'.len_utf8()], "\u{b0}\u{b0}\u{b0}\u{b0}");
        assert_eq!(
            s[1].chars().take(2).collect::<String>(),
            "\u{b0}]",
            "the control"
        );
    });
}

/// **An operation is done on both screens**: on the alternate screen DECFRA
/// fills and DECCRA copies there, and the primary keeps its text.
#[test]
fn rectangle_operations_act_on_the_alternate_screen() {
    let bytes = format!(
        "{}\x1b[?1049h\x1b[42;1;1;2;3$x\x1b[1;1;1;3;1;4;1;1$v",
        fill()
    );
    after(&bytes, |t| {
        let s = rows_of(t);
        assert_eq!(&s[0][..4], "***.");
        assert_eq!(
            &s[3][..4],
            "***.",
            "the copy landed on the alternate screen"
        );
    });
    after(&format!("{bytes}\x1b[?1049l"), |t| {
        let s = rows_of(t);
        assert_eq!(s[0], "aaaaaaaaaaaaaaaaaaaa");
        assert_eq!(s[3], "dddddddddddddddddddd");
    });
}

/// **DECFRA and DECCRA free an image they write over, as text written over
/// one frees it; DECERA and DECSERA leave it, as ED and EL do.** xterm
/// clears the graphics under all four (`ScrnFillRectangle`,
/// `ScrnWipeRectangle`); baud keeps its own split between writing and
/// erasing.
#[test]
fn a_fill_or_a_copy_over_an_image_frees_it_and_an_erase_does_not() {
    const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    let placed = |ops: &str| {
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4 * (64 * 48 + 64)));
        let mut t = Term::new(s.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        support::feed(&mut t, b"\x1b[3;3H");
        support::feed(&mut t, TINY);
        support::feed(&mut t, ops.as_bytes());
        t.images().placements().count()
    };
    assert_eq!(placed(""), 1, "the image is placed at row 3, column 3");
    assert_eq!(placed("\x1b[42;3;3;3;3$x"), 0, "DECFRA over it frees it");
    assert_eq!(
        placed("\x1b[1;1;1;3;1;3;1;1$v"),
        0,
        "and so does a DECCRA onto it"
    );
    assert_eq!(placed("\x1b[42;5;5;5;5$x"), 1, "a fill elsewhere leaves it");
    assert_eq!(placed("\x1b[3;3;3;3$z"), 1, "DECERA leaves it");
    assert_eq!(placed("\x1b[3;3;3;3${"), 1, "and DECSERA");
}

/// **A fill, a copy or an erase on a selection's rows reaches the watch on
/// them, and a change of attributes does not**: the owner's B23 drops a
/// selection for text written or erased, and xterm's `ScrnMarkRectangle`
/// keeps one.
#[test]
fn rectangle_writes_reach_a_selection_and_attribute_changes_do_not() {
    for (op, want) in [
        ("\x1b[42;2;1;2;3$x", true),
        ("\x1b[1;1;1;3;1;2;1;1$v", true),
        ("\x1b[2;1;2;3$z", true),
        ("\x1b[2;1;2;3${", true),
        ("\x1b[2;1;2;3;1$r", false),
        ("\x1b[2;1;2;3;1$t", false),
    ] {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        support::feed(&mut t, fill().as_bytes());
        t.watch_rows(Some((Row::from_index(1), Row::from_index(1))));
        support::feed(&mut t, op.as_bytes());
        assert_eq!(t.watched_rows_changed(), want, "{op:?}");
    }
}
