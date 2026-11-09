//! Double-width and double-height lines: `ESC # 3`, `# 4`, `# 5`, `# 6`.
//!
//! `[[source: dec-std-070]]` § 5.6.1 (p. 5-95): a line is single-width,
//! double-width, or the top or the bottom half of a double-height line, and
//! a line not single-width "doubles the character cell width within the
//! line, but does not affect the addressing of characters in the line ...
//! character positions cannot be addressed which are more than half the
//! current value of the right edge of the display". DECDWL and DECDHL empty
//! what lies past the line's new end and pull the cursor back to it (pp.
//! 5-97 to 5-100); DECSWL loses nothing (p. 5-96). `end_of_line` (p. 5-209)
//! is the width halved on any line not single-width, and every move, the
//! write and its wrap stop there. A scroll carries the rendition with its
//! line and brings in a single-width line (pp. 5-210 and 5-211); ED sets a
//! line it erases whole to single-width and EL leaves it (pp. 5-154 and
//! 5-156); DECLRMM set resets every line and refuses DECDWL and DECDHL (p.
//! 5-29).

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::ImageBudget;

/// The DEC screen: 80 columns, so a double-width line holds 40.
const VT: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

fn text(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y))
        .expect("a row on the screen")
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or('.')
        })
        .collect()
}

fn size(t: &Term<'_>, y: usize) -> LineRendition {
    t.line_rendition(Row::from_index(y))
        .expect("a row on the screen")
}

fn cursor(t: &Term<'_>) -> (usize, usize) {
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn on<R>(bytes: &[u8], f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    support::render(&VT, bytes, f)
}

fn dots(n: usize) -> String {
    ".".repeat(n)
}

/// `n` letters of the alphabet over and over, from `first`'s case.
fn letters(n: usize, upper: bool) -> String {
    let abc = match upper {
        true => "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        false => "abcdefghijklmnopqrstuvwxyz",
    };
    abc.chars().cycle().take(n).collect()
}

/// Each control sets the line the cursor is on, and only that line.
#[test]
fn each_control_sets_the_cursor_line() {
    on(
        b"\x1b[2H\x1b#6\x1b[3H\x1b#3\x1b[4H\x1b#4\x1b[5H\x1b#6\x1b#5",
        |t| {
            assert_eq!(size(t, 0), LineRendition::Single);
            assert_eq!(size(t, 1), LineRendition::DoubleWidth);
            assert_eq!(size(t, 2), LineRendition::DoubleTop);
            assert_eq!(
                size(t, 3),
                LineRendition::DoubleBottom,
                "DECDHLB is the bottom half, as its description says"
            );
            assert_eq!(size(t, 4), LineRendition::Single, "DECSWL undoes DECDWL");
            assert_eq!(t.unsupported, 0, "none is counted unsupported any more");
        },
    );
}

/// "Setting this attribute causes the length of the Active Line to be
/// reduced by half": fifty characters wrap after the fortieth.
#[test]
fn a_double_width_line_wraps_at_its_half() {
    let fifty = "A".repeat(50);
    on(&[b"\x1b#6".as_slice(), fifty.as_bytes()].concat(), |t| {
        assert_eq!(text(t, 0), format!("{}{}", "A".repeat(40), dots(40)));
        assert_eq!(text(t, 1), format!("{}{}", "A".repeat(10), dots(70)));
        assert_eq!(cursor(t), (1, 10));
    });
}

/// Forty characters fill the line and leave the wrap pending on its last
/// column, which `CPR` names; the forty-first wraps.
#[test]
fn the_pending_wrap_is_on_the_last_column_of_the_half() {
    let forty = "B".repeat(40);
    on(
        &[b"\x1b#6".as_slice(), forty.as_bytes(), b"\x1b[6n"].concat(),
        |t| {
            assert_eq!(cursor(t), (0, 39));
            assert_eq!(t.reply(), b"\x1b[1;40R");
            assert_eq!(t.wrap_pending(), WrapPending::Yes);
        },
    );
}

/// With autowrap reset the cursor stays on the half's last column and the
/// characters overwrite it.
#[test]
fn without_autowrap_the_half_s_last_column_takes_the_overflow() {
    let fifty = letters(50, false).into_bytes();
    on(&[b"\x1b[?7l\x1b#6".as_slice(), &fifty].concat(), |t| {
        let row = text(t, 0);
        assert_eq!(&row[..39], std::str::from_utf8(&fifty[..39]).unwrap());
        assert_eq!(&row[39..40], "x", "the fiftieth character, on column 40");
        assert_eq!(&row[40..], dots(40));
        assert_eq!(text(t, 1), dots(80));
        assert_eq!(cursor(t), (0, 39));
    });
}

/// "Setting this attribute causes data which is beyond the end of the Active
/// Line after execution of the control to be lost" (DECDWL note 2), and
/// DECSWL brings nothing back (note 2 of DECSWL keeps what the line holds).
#[test]
fn decdwl_loses_what_lies_past_the_half_and_decswl_does_not_bring_it_back() {
    let sixty = letters(60, false).into_bytes();
    on(&[sixty.as_slice(), b"\x1b#6\x1b#5"].concat(), |t| {
        let row = text(t, 0);
        assert_eq!(&row[..40], std::str::from_utf8(&sixty[..40]).unwrap());
        assert_eq!(&row[40..], dots(40), "the twenty past the half are gone");
    });
    // The same for both halves of a double-height line.
    for set in [b"\x1b#3", b"\x1b#4"] {
        on(&[sixty.as_slice(), set, b"\x1b#5"].concat(), |t| {
            assert_eq!(&text(t, 0)[40..], dots(40));
        });
    }
}

/// "if (active_position.column > end_of_line(...)) active_position.column =
/// end_of_line(...)" (pp. 5-98 and 5-100).
#[test]
fn decdwl_pulls_the_cursor_back_to_the_half() {
    on(b"\x1b[1;61H\x1b#6", |t| assert_eq!(cursor(t), (0, 39)));
    on(b"\x1b[1;21H\x1b#6", |t| assert_eq!(cursor(t), (0, 20)));
    let eighty = "c".repeat(80);
    // From the pending wrap past the screen's last column too.
    on(&[eighty.as_bytes(), b"\x1b#3"].concat(), |t| {
        assert_eq!(cursor(t), (0, 39));
        assert_eq!(t.wrap_pending(), WrapPending::No);
    });
}

/// "character positions cannot be addressed which are more than half the
/// current value of the right edge of the display": `CUP`, `CUF`, `CHA`,
/// `HPA`, `HPR` and `HT` stop on the half's last column.
#[test]
fn every_move_stops_at_the_half() {
    for (name, mv) in [
        ("CUP", b"\x1b[1;70H".as_slice()),
        ("HVP", b"\x1b[1;70f"),
        ("CUF", b"\x1b[60C"),
        ("CHA", b"\x1b[70G"),
        ("HPA", b"\x1b[70`"),
        ("HPR", b"\x1b[70a"),
        ("HT", b"\t\t\t\t\t\t\t\t\t"),
        ("CHT", b"\x1b[9I"),
    ] {
        on(&[b"\x1b#6".as_slice(), mv].concat(), |t| {
            assert_eq!(cursor(t), (0, 39), "{name} on a double-width line");
        });
    }
}

/// Moving onto a double-width line from a column past its half puts the
/// cursor on its last column: CUU, CUD, IND, RI, LF and CUP all end with
/// that clamp (pp. 5-41 to 5-60).
#[test]
fn a_move_onto_a_double_line_lands_inside_it() {
    let setup = b"\x1b[5H\x1b#6\x1b[10H\x1b#3\x1b[11H\x1b#4";
    for (name, mv, want) in [
        ("CUD", b"\x1b[4;70H\x1b[B".as_slice(), (4, 39)),
        ("CUU", b"\x1b[6;70H\x1b[A", (4, 39)),
        ("LF", b"\x1b[4;70H\n", (4, 39)),
        ("IND", b"\x1b[4;70H\x1bD", (4, 39)),
        ("RI", b"\x1b[6;70H\x1bM", (4, 39)),
        ("VPA", b"\x1b[1;70H\x1b[10d", (9, 39)),
        ("CUP", b"\x1b[11;75H", (10, 39)),
    ] {
        on(&[setup.as_slice(), mv].concat(), |t| {
            assert_eq!(cursor(t), want, "{name} onto a double line");
        });
    }
    // And back onto a single line the column is its own again.
    on(&[setup.as_slice(), b"\x1b[5;70H\x1b[6;70H"].concat(), |t| {
        assert_eq!(cursor(t), (5, 69));
    });
}

/// `ICH` and `DCH` shift within the line, not into the half past it: the
/// "effective right boundary" is `end_of_line` (pp. 5-143 and 5-145). A
/// character pushed off the line's end is lost, and DECSWL shows nothing past
/// it.
#[test]
fn ich_and_dch_shift_within_the_half() {
    let forty = letters(40, true).into_bytes();
    on(
        &[b"\x1b#6".as_slice(), &forty, b"\x1b[1;1H\x1b[3@\x1b#5"].concat(),
        |t| {
            let row = text(t, 0);
            assert_eq!(&row[..3], "...");
            assert_eq!(&row[3..40], std::str::from_utf8(&forty[..37]).unwrap());
            assert_eq!(&row[40..], dots(40), "nothing went into the hidden half");
        },
    );
    on(
        &[b"\x1b#6".as_slice(), &forty, b"\x1b[1;1H\x1b[3P\x1b#5"].concat(),
        |t| {
            let row = text(t, 0);
            assert_eq!(&row[..37], std::str::from_utf8(&forty[3..]).unwrap());
            assert_eq!(&row[37..], dots(43), "blanks come in at the half's end");
        },
    );
    // Insert mode writes within the line too.
    on(
        &[
            b"\x1b#6".as_slice(),
            &forty,
            b"\x1b[1;1H\x1b[4hXY\x1b[4l\x1b#5",
        ]
        .concat(),
        |t| {
            let row = text(t, 0);
            assert_eq!(&row[..2], "XY");
            assert_eq!(&row[2..40], std::str::from_utf8(&forty[..38]).unwrap());
            assert_eq!(&row[40..], dots(40));
        },
    );
}

/// "The line renditon is not affected by execution of this control" (EL note
/// 1, p. 5-154), nor by DECSEL and DECSED (their note 3, pp. 5-159 and
/// 5-162).
#[test]
fn el_and_the_selective_erases_keep_the_rendition() {
    for el in [
        b"\x1b[K".as_slice(),
        b"\x1b[1K",
        b"\x1b[2K",
        b"\x1b[?2K",
        b"\x1b[?2J",
        b"\x1b[5X",
    ] {
        on(
            &[b"\x1b[3H\x1b#6abc\x1b[3;1H".as_slice(), el].concat(),
            |t| {
                assert_eq!(size(t, 2), LineRendition::DoubleWidth, "{el:?}");
            },
        );
    }
}

/// "The Line Renditions of all lines completely erased ... are set to
/// single-width" (ED note 1, p. 5-156): ED 2 every line, ED 0 the lines below
/// and the cursor's from its first column, ED 1 the lines above and the
/// cursor's through its last (pp. 5-157 and 5-158; a product that did not
/// was granted a waiver for it, p. C-40).
#[test]
fn ed_sets_a_line_it_erases_whole_to_single_width() {
    let all = b"\x1b[2H\x1b#6\x1b[3H\x1b#6\x1b[4H\x1b#6";
    on(&[all.as_slice(), b"\x1b[2J"].concat(), |t| {
        for y in 1..4 {
            assert_eq!(size(t, y), LineRendition::Single, "ED 2, line {y}");
        }
    });
    on(&[all.as_slice(), b"\x1b[3;1H\x1b[J"].concat(), |t| {
        assert_eq!(size(t, 1), LineRendition::DoubleWidth, "above the cursor");
        assert_eq!(
            size(t, 2),
            LineRendition::Single,
            "the cursor's, from column 1"
        );
        assert_eq!(size(t, 3), LineRendition::Single, "below");
    });
    on(&[all.as_slice(), b"\x1b[3;5H\x1b[J"].concat(), |t| {
        assert_eq!(
            size(t, 2),
            LineRendition::DoubleWidth,
            "the cursor's, from column 5"
        );
        assert_eq!(size(t, 3), LineRendition::Single);
    });
    on(&[all.as_slice(), b"\x1b[3;40H\x1b[1J"].concat(), |t| {
        assert_eq!(size(t, 1), LineRendition::Single, "above");
        assert_eq!(
            size(t, 2),
            LineRendition::Single,
            "the cursor's, through its last column"
        );
        assert_eq!(size(t, 3), LineRendition::DoubleWidth, "below");
    });
    on(&[all.as_slice(), b"\x1b[3;39H\x1b[1J"].concat(), |t| {
        assert_eq!(
            size(t, 2),
            LineRendition::DoubleWidth,
            "the cursor's, short of its last"
        );
    });
}

/// "Line and Character Renditions are scrolled with the data. A new
/// single-width line with all character positions empty is inserted" (§
/// 5.14.2 and § 5.14.3, pp. 5-210 and 5-211): the full screen, a region, IL
/// and DL, SU and SD.
#[test]
fn a_scroll_carries_the_rendition_and_brings_in_single_width() {
    // The whole screen: line 24 double-width, one LF at the bottom.
    on(b"\x1b[24H\x1b#6dw\n", |t| {
        assert_eq!(
            size(t, 22),
            LineRendition::DoubleWidth,
            "moved up with its text"
        );
        assert_eq!(size(t, 23), LineRendition::Single, "the new line");
        assert_eq!(&text(t, 22)[..2], "dw");
        let sb = t.scrollback();
        assert_eq!(sb.rendition(Back::NEWEST), Some(LineRendition::Single));
    });
    // A line scrolled off the top keeps its size in the history.
    on(b"\x1b#6top\x1b[24H\n", |t| {
        let sb = t.scrollback();
        assert_eq!(sb.rendition(Back::NEWEST), Some(LineRendition::DoubleWidth));
    });
    // A region: RI at its top moves the line down and brings in a single.
    on(b"\x1b[5H\x1b#6\x1b[5;10r\x1b[5H\x1bM", |t| {
        assert_eq!(size(t, 4), LineRendition::Single);
        assert_eq!(size(t, 5), LineRendition::DoubleWidth);
    });
    // LF at a region's bottom moves it up.
    on(b"\x1b[7H\x1b#6\x1b[5;10r\x1b[10H\n", |t| {
        assert_eq!(size(t, 5), LineRendition::DoubleWidth);
        assert_eq!(size(t, 6), LineRendition::Single);
        assert_eq!(size(t, 9), LineRendition::Single);
    });
    // IL and DL.
    on(b"\x1b[5H\x1b#6\x1b[3H\x1b[2L", |t| {
        assert_eq!(size(t, 4), LineRendition::Single);
        assert_eq!(size(t, 6), LineRendition::DoubleWidth);
        assert_eq!(size(t, 2), LineRendition::Single, "the inserted lines");
    });
    on(b"\x1b[5H\x1b#6\x1b[3H\x1b[2M", |t| {
        assert_eq!(size(t, 2), LineRendition::DoubleWidth);
        assert_eq!(size(t, 4), LineRendition::Single);
    });
    // SU and SD.
    on(b"\x1b[5H\x1b#6\x1b[2S", |t| {
        assert_eq!(size(t, 2), LineRendition::DoubleWidth);
        assert_eq!(size(t, 4), LineRendition::Single);
    });
    on(b"\x1b[5H\x1b#6\x1b[2T", |t| {
        assert_eq!(size(t, 6), LineRendition::DoubleWidth);
        assert_eq!(size(t, 4), LineRendition::Single);
    });
    // A newline keeps the line it leaves as it is.
    on(b"\x1b#6ab\r\n", |t| {
        assert_eq!(size(t, 0), LineRendition::DoubleWidth)
    });
}

/// vttest's double-size box, closing with "Exactly half of the box should
/// remain": twelve RIs at the top of a region from line 8 push the box's
/// first eight lines down and the rest off the bottom.
#[test]
fn twelve_reverse_indexes_leave_half_of_a_double_height_box() {
    let mut b = Vec::new();
    for y in 8..=17 {
        b.extend_from_slice(format!("\x1b[{y};1H").as_bytes());
        b.extend_from_slice(if y % 2 == 0 { b"\x1b#3" } else { b"\x1b#4" });
        b.extend_from_slice(format!("line{y}").as_bytes());
    }
    b.extend_from_slice(b"\x1b[8;24r\x1b[8;1H");
    for _ in 0..12 {
        b.extend_from_slice(b"\x1bM");
    }
    b.extend_from_slice(b"\x1b[r");
    on(&b, |t| {
        for y in 7..19 {
            assert_eq!(
                size(t, y),
                LineRendition::Single,
                "line {} came in single",
                y + 1
            );
        }
        for (k, y) in (19..24).enumerate() {
            let want = if k % 2 == 0 {
                LineRendition::DoubleTop
            } else {
                LineRendition::DoubleBottom
            };
            assert_eq!(
                size(t, y),
                want,
                "line {} carries line{}'s size",
                y + 1,
                8 + k
            );
            assert!(
                text(t, y).starts_with(&format!("line{}.", 8 + k)),
                "line {}",
                y + 1
            );
        }
    });
}

/// DECLRMM set: "all line attributes currently in Page Memory for the Session
/// will be reset to Single Wide, Single High, and DECDWL and DECDHL escape
/// sequences ... will be ignored" (p. 5-29, and DECDWL note 3, DECDHL note
/// 4).
#[test]
fn declrmm_resets_every_line_and_refuses_the_double_controls() {
    on(b"\x1b[2H\x1b#6\x1b[3H\x1b#3\x1b[?69h", |t| {
        assert_eq!(size(t, 1), LineRendition::Single);
        assert_eq!(size(t, 2), LineRendition::Single);
    });
    on(b"\x1b[?69h\x1b[2H\x1b#6\x1b[3H\x1b#4\x1b[4H\x1b#3", |t| {
        for y in 1..4 {
            assert_eq!(
                size(t, y),
                LineRendition::Single,
                "refused on line {}",
                y + 1
            );
        }
    });
    // DECSWL still operates; and reset, the controls are taken again.
    on(b"\x1b[?69h\x1b[?69l\x1b[2H\x1b#6", |t| {
        assert_eq!(size(t, 1), LineRendition::DoubleWidth);
    });
}

/// DECALN (p. D-19), RIS (DEC STD 070-4's initialization) and DECCOLM (p.
/// 5-72) set every line single-width.
#[test]
fn decaln_ris_and_deccolm_reset_every_line() {
    for reset in [b"\x1b#8".as_slice(), b"\x1bc", b"\x1b[?40h\x1b[?3h"] {
        on(
            &[b"\x1b[2H\x1b#6\x1b[3H\x1b#3\x1b[4H\x1b#4".as_slice(), reset].concat(),
            |t| {
                for y in 0..t.geometry().1.index() {
                    assert_eq!(
                        size(t, y),
                        LineRendition::Single,
                        "{reset:?} line {}",
                        y + 1
                    );
                }
            },
        );
    }
}

/// Each screen has its own lines: the alternate screen starts single-width
/// and the primary's sizes come back with it.
#[test]
fn each_screen_keeps_its_own_renditions() {
    on(b"\x1b[2H\x1b#6\x1b[?1049h", |t| {
        assert_eq!(
            size(t, 1),
            LineRendition::Single,
            "the alternate screen is cleared"
        );
    });
    on(b"\x1b[2H\x1b#6\x1b[?1049h\x1b[3H\x1b#3\x1b[?1049l", |t| {
        assert_eq!(size(t, 1), LineRendition::DoubleWidth);
        assert_eq!(size(t, 2), LineRendition::Single);
    });
    // `?47` shows the alternate screen as it was left, sizes and all.
    on(b"\x1b[?47h\x1b[3H\x1b#3\x1b[?47l\x1b[?47h", |t| {
        assert_eq!(size(t, 2), LineRendition::DoubleTop);
    });
}

/// The rendition rides in the row's mark, so the mark keeps its promise
/// under it: the mark is raised to the width as the line is sized, so a line
/// sized over a low mark and written again is erased whole -- by ED, which
/// stops at a mark when the row ends in the blank, and by a scroll that
/// brings the row back in on the alternate screen, whose ring is the screen.
#[test]
fn an_erase_after_a_rendition_leaves_nothing_stale() {
    on(b"\x1b[2J\x1b#6abcdefghij\x1b[2J", |t| {
        assert_eq!(text(t, 0), dots(80));
        assert_eq!(size(t, 0), LineRendition::Single);
    });
    let mut b = b"\x1b[?1049h\x1b#6abcdefghij\x1b[24H".to_vec();
    b.extend(std::iter::repeat_n(b'\n', 24));
    on(&b, |t| {
        for y in 0..24 {
            assert_eq!(text(t, y), dots(80), "line {}", y + 1);
            assert_eq!(size(t, y), LineRendition::Single, "line {}", y + 1);
        }
    });
    on(b"\x1b#6\x1b[2Kabcdefghij\x1b[1;1H\x1b[K", |t| {
        assert_eq!(text(t, 0), dots(80));
        assert_eq!(size(t, 0), LineRendition::DoubleWidth);
    });
}

/// The screen does not depend on where the reads fell.
#[test]
fn the_controls_are_chunk_invariant() {
    let bytes = b"\x1b[3H\x1b#6hello world\x1b[5H\x1b#3top\x1b[6H\x1b#4bottom\x1b[1;70H\x1b[3;70Hz";
    let whole = support::render(&VT, bytes, |t| {
        (
            support::screen(t),
            cursor(t),
            (0..24).map(|y| size(t, y)).collect::<Vec<_>>(),
        )
    });
    for n in 1..=bytes.len() {
        let parts = support::with_term(&VT, |t| {
            for chunk in bytes.chunks(n) {
                support::feed(t, chunk);
            }
            (
                support::screen(t),
                cursor(t),
                (0..24).map(|y| size(t, y)).collect::<Vec<_>>(),
            )
        });
        assert!(parts == whole, "reads of {n} bytes");
    }
}

fn resize(t: &mut Term<'_>, rows: u16, cols: u16) {
    assert_eq!(
        t.resize(&WinSize {
            ws_row: rows,
            ws_col: cols
        }),
        Resized::Ok
    );
}

/// **A width change keeps a sized line one row, whole.** No text resizes a
/// page, so this is a choice: never joined to the rows round it, its
/// characters past a narrower half kept unseen and shown again by the
/// growth back.
#[test]
fn a_reflow_keeps_a_sized_line_whole_and_on_its_own() {
    let forty = letters(40, false);
    let long: String = "w".repeat(100);
    let bytes = format!("{long}\r\n\x1b#6{forty}\r\n\x1b#3top\r\n\x1b#4top\r\nafter");
    support::with_term(&VT, |t| {
        support::feed(t, bytes.as_bytes());
        // The 100 w's wrapped onto two rows; the sized line follows.
        assert_eq!(size(t, 2), LineRendition::DoubleWidth);
        resize(t, 24, 60);
        let at = (0..24)
            .find(|&y| size(t, y) == LineRendition::DoubleWidth)
            .expect("the double-width line survives");
        assert_eq!(&text(t, at)[..30], &forty[..30], "its first thirty show");
        assert_eq!(
            &text(t, at - 1)[..40],
            "w".repeat(40),
            "the wrapped line before it is not joined to it"
        );
        assert_eq!(size(t, at + 1), LineRendition::DoubleTop);
        assert_eq!(
            size(t, at + 2),
            LineRendition::DoubleBottom,
            "the pair stays a pair"
        );
        assert!(text(t, at + 3).starts_with("after"));
        resize(t, 24, 80);
        let at = (0..24)
            .find(|&y| size(t, y) == LineRendition::DoubleWidth)
            .expect("the double-width line survives");
        assert_eq!(&text(t, at)[..40], forty, "the growth back shows all forty");
        assert_eq!(&text(t, at - 1)[..20], "w".repeat(20));
    });
}

/// The cursor on a sized line stays inside its half when the width narrows.
#[test]
fn a_narrower_width_keeps_the_cursor_inside_the_half() {
    support::with_term(&VT, |t| {
        support::feed(t, b"\x1b[24H\x1b#6\x1b[24;40H");
        resize(t, 24, 50);
        assert_eq!(cursor(t).1, 24, "the last column of a 25-character line");
        support::feed(t, b"xyz");
        assert_eq!(cursor(t).0, 23, "the wrap went to a new line at the bottom");
    });
}

/// The alternate screen keeps a sized line's rendition through a width
/// change both ways.
#[test]
fn the_alternate_screen_keeps_renditions_through_a_resize() {
    support::with_term(&VT, |t| {
        support::feed(t, b"\x1b[?1049h\x1b[3H\x1b#6abc");
        resize(t, 24, 60);
        assert_eq!(size(t, 2), LineRendition::DoubleWidth);
        resize(t, 24, 80);
        assert_eq!(size(t, 2), LineRendition::DoubleWidth);
        assert!(text(t, 2).starts_with("abc"));
        support::feed(t, b"\x1b[3;1H\x1b[2K");
        assert_eq!(
            text(t, 2),
            dots(80),
            "an erase after the resizes leaves nothing stale"
        );
    });
}

/// **A sized line's mark rises with the width**, on both screens: sized
/// at 30 columns, its half is 15 wide, and grown to 80 its half is 40. A
/// mark left at the narrow width would let an ED that stops at the mark
/// skip the characters written past it.
#[test]
fn a_growth_past_twice_the_width_raises_a_sized_line_s_mark() {
    let forty = "q".repeat(40);
    for screen in [b"".as_slice(), b"\x1b[?1049h"] {
        support::with_term(&VT, |t| {
            support::feed(t, screen);
            resize(t, 24, 30);
            support::feed(t, b"\x1b[2J\x1b[3H\x1b#6");
            resize(t, 24, 80);
            let at = (0..24)
                .find(|&y| size(t, y) == LineRendition::DoubleWidth)
                .expect("the line keeps its rendition");
            support::feed(t, format!("\x1b[{};1H{forty}\x1b[2J", at + 1).as_bytes());
            for y in 0..24 {
                assert_eq!(text(t, y), dots(80), "{screen:?} line {}", y + 1);
            }
        });
    }
}

/// **A run that wraps off a double line writes on at the next line's
/// width**, at every read size. The fence a double line leaves is read low
/// on the line after it until a cold path asks the line's own; a run of
/// text wrapped off the half and, in one read, kept the half's fence for
/// the next line and wrapped again at its tenth column (found by
/// `the_screen_does_not_depend_on_where_the_reads_fell`).
#[test]
fn a_run_wrapped_off_a_double_line_takes_the_next_line_s_width() {
    let ws = WinSize {
        ws_row: 50,
        ws_col: 20,
    };
    let prog = [
        "\u{4e16}\u{4e16}\u{4e16}\u{4e16}",
        "\u{4e16}\u{4e16}\u{4e16}",
        "\r\n",
        "\u{2500}\u{2500}\u{2500}\u{2500}",
        "\u{4e16}\u{4e16}\u{4e16}\u{4e16}",
        "\u{e9}\u{e9}\u{e9}",
        "\x1b#6",
        "the quick brown fox",
    ]
    .concat()
    .into_bytes();
    for read in 1..=prog.len() {
        support::with_term(&ws, |t| {
            for chunk in prog.chunks(read) {
                support::feed(t, chunk);
            }
            assert_eq!(size(t, 1), LineRendition::DoubleWidth);
            let row2: String = text(t, 2).chars().take(18).collect();
            assert_eq!(row2, "he quick brown fox", "reads of {read}");
            assert_eq!(cursor(t), (2, 18), "reads of {read}");
        });
    }
}

/// **A sized line narrowed below what it holds, and grown back**, on both
/// screens: forty characters on a double-width line at 80 columns, then 30,
/// where its cells and its raised mark lie past the live edge, then 80
/// again. The line keeps its rendition, and its text as each screen keeps
/// text past a narrowed edge: the primary all forty, the alternate screen
/// the thirty inside the narrow width, since a growth there blanks the
/// columns it reveals (`View::widened`). An erase after leaves nothing.
#[test]
fn a_sized_line_narrowed_below_its_text_comes_back_whole() {
    let forty = letters(40, true);
    for screen in [b"".as_slice(), b"\x1b[?1049h"] {
        support::with_term(&VT, |t| {
            support::feed(t, screen);
            support::feed(t, format!("\x1b[12H\x1b#6{forty}\x1b[1;1H").as_bytes());
            resize(t, 24, 30);
            let at = (0..24)
                .find(|&y| size(t, y) == LineRendition::DoubleWidth)
                .expect("the line keeps its rendition at 30");
            assert_eq!(
                &text(t, at)[..15],
                &forty[..15],
                "{screen:?}: its first fifteen show"
            );
            resize(t, 24, 80);
            let at = (0..24)
                .find(|&y| size(t, y) == LineRendition::DoubleWidth)
                .expect("the line keeps its rendition at 80");
            let kept = match screen.is_empty() {
                true => forty.clone(),
                false => format!("{}{}", &forty[..30], dots(10)),
            };
            assert_eq!(&text(t, at)[..40], kept, "{screen:?}: what comes back");
            support::feed(t, format!("\x1b[{};1H\x1b[2K\x1b[2J", at + 1).as_bytes());
            for y in 0..24 {
                assert_eq!(text(t, y), dots(80), "{screen:?} line {}", y + 1);
            }
        });
    }
}

/// **The line after a double line takes its own width for every writer**:
/// the bulk text run, two-byte characters, wide characters and a run of
/// one character at a time each fill the single-width line below a
/// double-width one to its eightieth column, whether the cursor came down
/// by a newline, a scroll at the bottom or a `CUP`.
#[test]
fn the_line_after_a_double_line_takes_its_own_width_for_every_writer() {
    let ascii = "x".repeat(80);
    let latin = "\u{e9}".repeat(80);
    let wide = "\u{4e16}".repeat(40);
    let one: String = (0..80).map(|_| "y\x1b[m").collect();
    for (name, run, cell) in [
        ("ascii", ascii.as_str(), 'x'),
        ("two-byte", latin.as_str(), '\u{e9}'),
        ("wide", wide.as_str(), '\u{4e16}'),
        ("one at a time", one.as_str(), 'y'),
    ] {
        for (how, lead) in [
            ("newline", "\x1b[5H\x1b#6\r\n"),
            ("scroll", "\x1b[24H\x1b#6\r\n"),
            ("CUP", "\x1b[5H\x1b#6\x1b[6;1H"),
        ] {
            on(format!("{lead}{run}").as_bytes(), |t| {
                let (y, _) = cursor(t);
                let row: Vec<char> = text(t, y).chars().filter(|c| *c == cell).collect();
                assert_eq!(
                    row.len(),
                    match cell {
                        '\u{4e16}' => 40,
                        _ => 80,
                    },
                    "{name} after a {how}: {:?}",
                    text(t, y)
                );
                assert_eq!(
                    cursor(t).1,
                    79,
                    "{name} after a {how}: the wrap pending on the last column"
                );
            });
        }
    }
}

/// **None of the four controls leaves a wrap pending**: STD 070's D.6.1
/// (pp. D-13 and D-14) lists DECSWL, DECDWL and both halves of DECDHL among
/// the controls that reset the Last Column Flag, so after one at a pending
/// wrap the next character lands on the column the wrap was pending on: the
/// line's last column, or for DECSWL on a double line the last column of its
/// old half, since DECSWL leaves the active position where it is (p. 5-96).
#[test]
fn the_controls_end_a_pending_wrap() {
    let forty = "b".repeat(40);
    for from in [b"\x1b#6", b"\x1b#3"] {
        let full = [from.as_slice(), forty.as_bytes(), b"\x1b#5"].concat();
        on(&[full.as_slice(), b"\x1b[6n"].concat(), |t| {
            assert_eq!(t.reply(), b"\x1b[1;40R", "{from:?} then DECSWL: CPR");
            assert_eq!(t.wrap_pending(), WrapPending::No, "{from:?}: no wrap");
        });
        on(&[full.as_slice(), b"Z"].concat(), |t| {
            let row = text(t, 0);
            assert_eq!(&row[38..41], "bZ.", "{from:?} then DECSWL: Z on 40");
            assert_eq!(text(t, 1), dots(80), "{from:?}: the next line untouched");
        });
    }
    let eighty = "a".repeat(80);
    on(&[eighty.as_bytes(), b"\x1b#5Z"].concat(), |t| {
        assert_eq!(&text(t, 0)[79..], "Z", "DECSWL on a full single-width line");
        assert_eq!(text(t, 1), dots(80), "nothing wrapped");
    });
    for set in [b"\x1b#6", b"\x1b#3", b"\x1b#4"] {
        on(
            &[b"\x1b#6".as_slice(), forty.as_bytes(), set, b"Z"].concat(),
            |t| {
                assert_eq!(
                    &text(t, 0)[39..40],
                    "Z",
                    "{set:?} on a full double-width line"
                );
                assert_eq!(text(t, 1), dots(80), "{set:?}: nothing wrapped");
            },
        );
    }
}

// ---- The rectangle operations over a sized line ------------------------
//
// Every one of DECCRA, DECFRA, DECERA, DECSERA, DECCARA and DECRARA carries
// the same note (`[[source: dec-std-070]]` pp. 5-169 to 5-176): "If the
// rectangular area contains both single and double width lines, the area
// affected may not appear rectangular on the display. For example if Pl=5
// and Pr=10, then all lines within the rectangular area, whether single or
// double width, will have character positions 5 through 10 affected." The
// columns are character positions, and on a line not single-width
// "character positions cannot be addressed which are more than half the
// current value of the right edge of the display" (p. 5-95): a rectangle
// stops at each line's own end. xterm's `validRect` reads the screen's
// width for every row and acts on a double line's hidden cells
// (`[[source: xterm-screen]]`).

/// A double-width line on row 3 holding forty letters, its last ten hidden
/// past the half a narrowing to 60 columns leaves, which a reflow keeps
/// unseen (`a_reflow_keeps_a_sized_line_whole_and_on_its_own`).
fn hidden_half(t: &mut Term<'_>) -> usize {
    support::feed(t, format!("\x1b[3H\x1b#6{}", letters(40, false)).as_bytes());
    resize(t, 24, 60);
    (0..24)
        .find(|&y| size(t, y) == LineRendition::DoubleWidth)
        .expect("the double-width line survives")
}

/// The sized line found again after the growth back to 80 columns.
fn grown(t: &mut Term<'_>) -> usize {
    resize(t, 24, 80);
    (0..24)
        .find(|&y| size(t, y) == LineRendition::DoubleWidth)
        .expect("the double-width line survives")
}

fn attrs_at(t: &Term<'_>, y: usize, x: usize) -> Attrs {
    let cell = t.row(Row::from_index(y)).expect("a row")[x];
    t.style_of(cell).attrs()
}

/// DECFRA fills a double line's forty positions and not the forty past.
#[test]
fn decfra_fills_a_double_line_to_its_end() {
    on(b"\x1b[3H\x1b#6\x1b[88;1;1;24;80$x", |t| {
        assert_eq!(text(t, 2), format!("{}{}", "X".repeat(40), dots(40)));
        assert_eq!(text(t, 1), "X".repeat(80), "a single line, all eighty");
    });
}

/// DECCRA's destination: "Text copied to the destination area take on the
/// line attributes of that area" and "only information which will appear
/// in valid character positions in the page will be copied" (p. 5-169).
#[test]
fn deccra_copies_onto_a_double_line_up_to_its_end() {
    let row = letters(80, false);
    let bytes = format!("{row}\x1b[3H\x1b#6\x1b[1;1;1;80;1;3;1;1$v");
    on(bytes.as_bytes(), |t| {
        assert_eq!(text(t, 2), format!("{}{}", &row[..40], dots(40)));
    });
}

/// DECCARA under DECSACE's rectangle turns the unoccupied positions inside
/// it to blanks (p. 5-177), the line's forty and no more.
#[test]
fn deccara_in_a_rectangle_blanks_a_double_line_to_its_end() {
    on(b"\x1b[3H\x1b#6ab\x1b[2*x\x1b[1;1;24;80;1$r", |t| {
        assert_eq!(text(t, 2), format!("ab{}{}", " ".repeat(38), dots(40)));
        assert!(attrs_at(t, 2, 39).has(Attrs::BOLD), "its last position");
        assert!(!attrs_at(t, 2, 40).has(Attrs::BOLD), "past its end");
    });
}

/// DECCARA's stream runs each line to its own end, so the hidden half of a
/// narrowed double line keeps its rendition.
#[test]
fn deccara_in_a_stream_stops_at_a_double_line_end() {
    support::with_term(&VT, |t| {
        hidden_half(t);
        support::feed(t, b"\x1b[1;1;24;60;1$r");
        let y = grown(t);
        assert!(attrs_at(t, y, 29).has(Attrs::BOLD), "the line's last");
        for x in 30..40 {
            assert!(!attrs_at(t, y, x).has(Attrs::BOLD), "hidden {x}");
        }
    });
}

/// DECRARA under the rectangle: the same end.
#[test]
fn decrara_stops_at_a_double_line_end() {
    support::with_term(&VT, |t| {
        hidden_half(t);
        support::feed(t, b"\x1b[2*x\x1b[1;1;24;60;7$t");
        let y = grown(t);
        assert!(attrs_at(t, y, 29).has(Attrs::REVERSE), "the line's last");
        for x in 30..40 {
            assert!(!attrs_at(t, y, x).has(Attrs::REVERSE), "hidden {x}");
        }
    });
}

/// DECERA leaves the positions past a double line's end as they were.
#[test]
fn decera_stops_at_a_double_line_end() {
    support::with_term(&VT, |t| {
        hidden_half(t);
        support::feed(t, b"\x1b[1;1;24;60$z");
        let y = grown(t);
        let abc = letters(40, false);
        assert_eq!(
            text(t, y),
            format!("{}{}{}", " ".repeat(30), &abc[30..], dots(40))
        );
    });
}

/// DECSERA likewise.
#[test]
fn decsera_stops_at_a_double_line_end() {
    support::with_term(&VT, |t| {
        hidden_half(t);
        support::feed(t, b"\x1b[1;1;24;60${");
        let y = grown(t);
        let abc = letters(40, false);
        assert_eq!(
            text(t, y),
            format!("{}{}{}", " ".repeat(30), &abc[30..], dots(40))
        );
    });
}

/// DECRQCRA sums a double line's own positions: a narrowed line's hidden
/// half counts nothing, the sum a line written to its thirty gets.
#[test]
fn decrqcra_sums_a_double_line_to_its_end() {
    let ask = b"\x1b[1;1;1;1;24;60*y";
    let hidden = support::with_term(&VT, |t| {
        hidden_half(t);
        support::feed(t, ask);
        t.reply().to_vec()
    });
    let thirty = WinSize {
        ws_row: 24,
        ws_col: 60,
    };
    let bytes = format!("\x1b[3H\x1b#6{}", letters(30, false));
    let shown = support::render(&thirty, &[bytes.as_bytes(), ask].concat(), |t| {
        t.reply().to_vec()
    });
    assert!(!shown.is_empty(), "answered");
    assert_eq!(hidden, shown);
}

/// DECCRA's source: "characters 5 through 10 from all lines within the
/// rectangular area will be copied" (p. 5-169), and a position past a
/// double line's end is none, so it is copied as unoccupied -- what a DEC
/// terminal's copy gives, whose cells there are always empty. A narrowed
/// line's hidden letters never reach the single-width line.
#[test]
fn deccra_reads_a_double_line_past_its_end_as_unoccupied() {
    support::with_term(&VT, |t| {
        let y = hidden_half(t);
        let to = y + 3;
        support::feed(
            t,
            format!("\x1b[{};1H{}", to + 1, "z".repeat(60)).as_bytes(),
        );
        support::feed(
            t,
            format!("\x1b[{0};1;{0};60;1;{1};1;1$v", y + 1, to + 1).as_bytes(),
        );
        let abc = letters(40, false);
        assert_eq!(text(t, to), format!("{}{}", &abc[..30], dots(30)));
    });
}

// ---- Images placed on a sized line ---------------------------------------
//
// An image is not text, and is not stretched with the line: "No other
// information is carried from the ANSI text state" (`[[source: dec-std-070]]`
// 070-9 § 11.1). It starts at the cursor's displayed place, "the upper-left
// corner of the ANSI text active position" (`[[source: vt3xx-sixel]]`), which
// on a double line is the doubled character field, "column 9 of the physical
// display" for character position 5 (p. 5-95); xterm places it at the
// single-width column of the character position
// (`[[source: xterm-graphics-c]]`, `charcol * FontWidth`).

/// A 24x6 red Sixel: three columns at an eight-pixel cell.
const SIXEL_24: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0!24~\x1b\\";

/// A terminal with image memory and an 8x16 cell.
fn with_images<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&VT).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &VT);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    f(&mut t)
}

fn placed_cols(t: &Term<'_>) -> Vec<usize> {
    t.images()
        .placements()
        .map(|(_, p)| p.col.index())
        .collect()
}

/// A Sixel begun at character position 9 of a double-width line is placed
/// at display column 17, its doubled field's left edge; on a single-width
/// line at column 9.
#[test]
fn a_sixel_on_a_double_line_starts_at_the_cursor_s_displayed_place() {
    for (set, want) in [("\x1b#6", 16), ("\x1b#3", 16), ("", 8)] {
        with_images(|t| {
            support::feed(t, format!("\x1b[3;1H{set}\x1b[3;9H").as_bytes());
            support::feed(t, SIXEL_24);
            assert_eq!(placed_cols(t), vec![want], "{set:?}");
        });
    }
}

/// A kitty placement likewise, and the cursor moves past it to the column
/// right of the image on the row it lands on: three display columns from
/// display column 17, and one row of image moves it down none, as kitty
/// moves it (the rows less one), so the double line's character position
/// 11, the first whose field starts right of the image. By the text's
/// "down by the number of rows" it went to column 20 of the single-width row
/// below.
#[test]
fn a_kitty_placement_on_a_double_line_starts_at_the_cursor_s_displayed_place() {
    // A 24x16 RGBA image, all red, base64 of 24 * 16 * 4 bytes.
    let px = [255u8, 0, 0, 255].repeat(24 * 16);
    let b64 = base64(&px);
    with_images(|t| {
        support::feed(t, b"\x1b[3;1H\x1b#6\x1b[3;9H");
        support::feed(
            t,
            format!("\x1b_Ga=T,f=32,s=24,v=16,i=5,c=3,r=1,q=2;{b64}\x1b\\").as_bytes(),
        );
        assert_eq!(placed_cols(t), vec![16]);
        assert_eq!(cursor(t), (2, 10), "right of the image, on its row");
    });
}

/// Text over the image frees it by the columns it covers on the display: a
/// character at position 11 of a double line covers display columns 21 and
/// 22, past a three-column image at 17, and frees nothing; one at 10 covers
/// 19 and 20 and frees it.
#[test]
fn text_over_an_image_on_a_double_line_frees_it_by_the_columns_it_covers() {
    with_images(|t| {
        support::feed(t, b"\x1b[3;1H\x1b#6\x1b[3;9H");
        support::feed(t, SIXEL_24);
        assert_eq!(placed_cols(t), vec![16]);
        support::feed(t, b"\x1b[3;11Hx");
        assert_eq!(placed_cols(t), vec![16], "past the image");
        support::feed(t, b"\x1b[3;10Hx");
        assert!(placed_cols(t).is_empty(), "over the image");
    });
}

/// **A delete by the cursor's cell takes every placement the cell
/// intersects** (`[[source: kitty-graphics]]`, "c or C Delete all
/// placements that intersect with the current cursor position"), and on a
/// double line the cell is two display columns. Counted from 1: a
/// four-column, three-row image at display columns 12 to 15, its rows
/// crossing a double line below; the cursor there at character position 6
/// covers display columns 11 and 12 and at 8 covers 15 and 16, so the
/// image goes, and at 5 (9 and 10) or 9 (17 and 18) it stays; on a
/// single-width line column 11 misses it and column 15 meets it.
#[test]
fn a_delete_by_the_cursor_on_a_double_line_takes_both_display_columns() {
    let px = [255u8, 0, 0, 255].repeat(32 * 48);
    let b64 = base64(&px);
    for (set, at, gone) in [
        ("\x1b#6", 6, true),
        ("\x1b#3", 6, true),
        ("\x1b#6", 8, true),
        ("\x1b#6", 5, false),
        ("\x1b#6", 9, false),
        ("", 11, false),
        ("", 15, true),
    ] {
        for d in ["c", "C"] {
            with_images(|t| {
                support::feed(t, format!("\x1b[3;1H{set}\x1b[1;12H").as_bytes());
                support::feed(
                    t,
                    format!("\x1b_Ga=T,f=32,s=32,v=48,i=5,c=4,r=3,C=1,q=2;{b64}\x1b\\").as_bytes(),
                );
                // Display column 12, index 11.
                assert_eq!(placed_cols(t), vec![11], "{set:?}");
                support::feed(
                    t,
                    format!("\x1b[3;{at}H\x1b_Ga=d,d={d},q=2\x1b\\").as_bytes(),
                );
                assert_eq!(placed_cols(t).is_empty(), gone, "{set:?} at {at}, d={d}");
            });
        }
    }
}

/// `?8452` leaves the cursor right of the image on its last row: on the
/// double line, the character position after display column 19.
#[test]
fn sixel_scrolls_right_counts_a_double_line_s_characters() {
    with_images(|t| {
        // The mode is recognized while DA lists Sixel.
        t.set_sixel_claim(baud_domain::SixelClaim::Yes);
        support::feed(t, b"\x1b[?8452h\x1b[3;1H\x1b#6\x1b[3;9H");
        support::feed(t, SIXEL_24);
        assert_eq!(cursor(t), (2, 10));
    });
}

/// Each placement's screen row and display column, in slot order.
fn placed_at(t: &Term<'_>) -> Vec<(i128, usize)> {
    let top = i128::from(t.image_origin(Rows::ZERO).top.value());
    t.images()
        .placements()
        .map(|(_, p)| (i128::from(p.line.value()) - top, p.col.index()))
        .collect()
}

/// A 32x48 RGBA kitty placement, four columns by three rows at the cursor,
/// the cursor left where it was (`C=1`).
fn kitty_32x48() -> String {
    let px = [255u8, 0, 0, 255].repeat(32 * 48);
    format!(
        "\x1b_Ga=T,f=32,s=32,v=48,i=5,c=4,r=3,C=1,q=2;{}\x1b\\",
        base64(&px)
    )
}

/// **IL and DL carry an image on a sized line with the line**: "Data in
/// the display is scrolled downward to the Bottom Margin" and the line's
/// rendition with it, State Affected naming both (`[[source: dec-std-070]]`
/// pp. 5-146 to 5-149). The image keeps the display column it began at,
/// so on the moved double line the cell at a character position meets it
/// by the display columns the cell covers there: position 11 (display
/// 21-22) misses an image from display column 17, position 10 (19-20)
/// meets it -- text over a Sixel frees it, and a kitty delete by the
/// cursor's cell (`d=c`) takes the placement. On each rendition, for a
/// Sixel and a kitty placement, by IL and by DL above the line.
#[test]
fn il_and_dl_carry_an_image_on_a_sized_line_with_its_line() {
    let kitty = kitty_32x48();
    for set in ["\x1b#6", "\x1b#3", "\x1b#4"] {
        for (image, name) in [(SIXEL_24, "sixel"), (kitty.as_bytes(), "kitty")] {
            for (op, to) in [("\x1b[2;1H\x1b[2L", 6), ("\x1b[2;1H\x1b[2M", 2)] {
                with_images(|t| {
                    support::feed(t, format!("\x1b[5;1H{set}\x1b[5;9H").as_bytes());
                    support::feed(t, image);
                    assert_eq!(placed_at(t), vec![(4, 16)], "{set:?} {name}");
                    support::feed(t, op.as_bytes());
                    assert_eq!(size(t, to), size_of(set), "{set:?} {op:?}: the rendition");
                    assert_eq!(
                        placed_at(t),
                        vec![(to as i128, 16)],
                        "{set:?} {name} {op:?}"
                    );
                    let at = |c: usize| match name {
                        "sixel" => format!("\x1b[{};{c}Hx", to + 1),
                        _ => format!("\x1b[{};{c}H\x1b_Ga=d,d=c,q=2\x1b\\", to + 1),
                    };
                    support::feed(t, at(11).as_bytes());
                    assert_eq!(placed_at(t).len(), 1, "{set:?} {name} {op:?}: past it");
                    support::feed(t, at(10).as_bytes());
                    assert!(placed_at(t).is_empty(), "{set:?} {name} {op:?}: over it");
                });
            }
        }
    }
}

fn size_of(set: &str) -> LineRendition {
    match set {
        "\x1b#6" => LineRendition::DoubleWidth,
        "\x1b#3" => LineRendition::DoubleTop,
        "\x1b#4" => LineRendition::DoubleBottom,
        _ => LineRendition::Single,
    }
}

/// **DECIC and DECDC leave an image where it is**, on a sized line as on
/// any: the kitty text moves images only when "scrolling the screen" and
/// gives erases "no effect on graphics" (`[[source: kitty-graphics]]`,
/// "Interaction with other terminal actions"), no DEC text puts a Sixel
/// under DECIC, and xterm and kitty move none. The text shifts under it.
#[test]
fn decic_and_decdc_leave_an_image_on_a_sized_line_where_it_is() {
    let kitty = kitty_32x48();
    let forty = letters(40, true);
    for (image, name) in [(SIXEL_24, "sixel"), (kitty.as_bytes(), "kitty")] {
        for (op, want) in [
            ("\x1b[3'}", format!("...{}", &forty[..37])),
            ("\x1b[3'~", forty[3..].to_string()),
        ] {
            with_images(|t| {
                support::feed(t, format!("\x1b[5;1H\x1b#6{forty}\x1b[5;9H").as_bytes());
                support::feed(t, image);
                assert_eq!(placed_at(t), vec![(4, 16)], "{name}");
                support::feed(t, format!("\x1b[5;1H{op}").as_bytes());
                assert_eq!(&text(t, 4)[..want.len()], want, "{name} {op:?}: the text");
                assert_eq!(placed_at(t), vec![(4, 16)], "{name} {op:?}: the image");
            });
        }
    }
}

/// **DECIC and DECDC shift every row of the region to the right margin**,
/// a sized line's included. Their texts say "The contents of the display
/// are shifted to the right from the active column to the right margin"
/// (DECIC, p. 5-150; DECDC, p. 5-151), are silent on a sized line and
/// leave the algorithms "to be supplied"; ICH's and DCH's algorithms bound
/// their shift by the line's end (pp. 5-143 and 5-145,
/// `ich_and_dch_shift_within_the_half`), and the scroll_left and
/// scroll_right routines of the horizontal-scrolling extension, which
/// DECFI and DECBI call, run every row to the right margin (§ 5.14.4 and §
/// 5.14.5, pp. 5-212 and 5-213). So three letters DECIC pushes past a
/// double line's fortieth position are kept past it and come back on
/// DECSWL, which "does not cause data contained in the Active Line to be
/// lost" (p. 5-96), and DECDC after it brings them back in. The one STD
/// 070 note on a sized line under a horizontal shift is note 1 of DECFI
/// and of DECBI (pp. 5-37 and 5-39), not of DECIC: "Lines of text are
/// shifted one column regardless of their line attributes. This means
/// double-wide and double-size lines will appear to shift twice as fast as
/// single-width lines." xterm shifts each row to its right margin
/// (`[[source: xterm-scroll]]`, `xtermColScroll`).
#[test]
fn decic_and_decdc_shift_a_sized_line_to_the_right_margin() {
    let forty = letters(40, true);
    on(
        format!("\x1b[2;1H\x1b#6{forty}\x1b[1;1H\x1b[3'}}\x1b[2;1H\x1b#5").as_bytes(),
        |t| assert_eq!(text(t, 1), format!("...{forty}{}", dots(37))),
    );
    on(
        format!("\x1b[2;1H\x1b#6{forty}\x1b[1;1H\x1b[3'}}\x1b[3'~").as_bytes(),
        |t| assert_eq!(text(t, 1), format!("{forty}{}", dots(40))),
    );
}

/// **DECFI and DECBI shift a sized line to the right margin too**, by
/// their own texts: note 1 of each, "Lines of text are shifted one column
/// regardless of their line attributes. This means double-wide and
/// double-size lines will appear to shift twice as fast as single-width
/// lines" (`[[source: dec-std-070]]` pp. 5-37 and 5-39), and their
/// algorithms call the scroll_left and scroll_right routines, which move
/// every row of the region "between the column designated and the Right
/// Margin (inclusive)" (§ 5.14.4 and § 5.14.5, pp. 5-212 and 5-213). So
/// DECBI at the left margin pushes a double line's fortieth letter past
/// its half, where it is kept, and DECFI at the right margin brings the
/// first of three DECIC pushed past it back into view. The cursor is on
/// row 1, single-width, for both.
#[test]
fn decfi_and_decbi_shift_a_sized_line_to_the_right_margin() {
    let forty = letters(40, true);
    on(
        format!("\x1b[2;1H\x1b#6{forty}\x1b[1;1H\x1b6").as_bytes(),
        |t| assert_eq!(text(t, 1), format!(".{forty}{}", dots(39)), "DECBI"),
    );
    on(
        format!("\x1b[2;1H\x1b#6{forty}\x1b[1;1H\x1b[3'}}\x1b[1;80H\x1b9").as_bytes(),
        |t| assert_eq!(text(t, 1), format!("..{forty}{}", dots(38)), "DECFI"),
    );
}

fn base64(data: &[u8]) -> String {
    const B: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            out.push(match k <= q.len() {
                true => char::from(B[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => '=',
            });
        }
    }
    out
}
