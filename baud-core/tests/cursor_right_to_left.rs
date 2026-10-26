//! **DECRLM, `CSI ? 34 h/l`** (`[[source: vt510-decrlm]]`): the cursor
//! writes right to left. Every expectation is the text's; `.` is an empty
//! cell. What a row written so draws is `baud-render/tests/strings.rs`.

mod support;

use baud_core::*;
use baud_domain::RowWritten;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

/// Row `r` of the screen and the cursor, after `bytes`.
fn after(bytes: &str) -> (Vec<String>, (usize, usize)) {
    support::with_term(&WS, |t| {
        support::feed(t, bytes.as_bytes());
        let rows = (0..4)
            .map(|r| {
                t.viewport()
                    .row(Row::from_index(r))
                    .iter()
                    .map(|c| match c.content {
                        x if x.is_empty() => '.',
                        x if x.is_wide_tail() => '~',
                        x => x
                            .as_codepoint()
                            .and_then(|cp| char::from_u32(cp.value()))
                            .unwrap_or('?'),
                    })
                    .collect()
            })
            .collect();
        let (r, c) = t.cursor();
        (rows, (r.index(), c.index()))
    })
}

/// DECRQM's answer for private mode `n` after `bytes`.
fn decrqm(bytes: &str, n: u16) -> String {
    support::with_term(&WS, |t| {
        support::feed(t, bytes.as_bytes());
        t.clear_reply();
        support::feed(t, format!("\x1b[?{n}$p").as_bytes());
        String::from_utf8_lossy(t.reply()).into_owned()
    })
}

const RLM: &str = "\x1b[?34h";

/// **DECRQM answers DECRLM**, reset at power-on and set once `?34 h` sets
/// it; `DECSTR` resets it ("Cursor direction DECRLM Reset", `[[source:
/// vt510-decstr]]`), and so does `RIS`. Until now `?34` was counted and
/// answered `0`.
#[test]
fn decrqm_answers_decrlm_and_decstr_resets_it() {
    assert_eq!(decrqm("", 34), "\x1b[?34;2$y");
    assert_eq!(decrqm(RLM, 34), "\x1b[?34;1$y");
    assert_eq!(decrqm(&format!("{RLM}\x1b[!p"), 34), "\x1b[?34;2$y");
    assert_eq!(decrqm(&format!("{RLM}\x1bc"), 34), "\x1b[?34;2$y");
}

/// **Each character is written at the cursor, which moves left**: "The
/// next character position is defined as the current cursor position minus
/// 1"; column numbering is unchanged, so the cells hold the text reversed,
/// and the row is marked as written so.
#[test]
fn a_character_is_written_and_the_cursor_moves_left() {
    let (rows, cursor) = after(&format!("{RLM}\x1b[1;10Habc"));
    assert_eq!(rows[0], ".......cba");
    assert_eq!(cursor, (0, 6));
    support::with_term(&WS, |t| {
        support::feed(t, format!("{RLM}\x1b[1;10Habc").as_bytes());
        let vp = t.view(Rows::ZERO);
        assert_eq!(vp.bidi(Row::ZERO).written(), RowWritten::RightToLeft);
        assert_eq!(vp.bidi(Row::from_index(1)).written(), RowWritten::InOrder);
    });
}

/// **With DECAWM set a character at column one wraps to the right margin
/// of the next line**; reset, it overwrites column one.
#[test]
fn at_column_one_it_wraps_to_the_right_margin_or_overwrites() {
    let (rows, cursor) = after(&format!("{RLM}\x1b[1;3Habcd"));
    assert_eq!(rows[0], "cba.......");
    assert_eq!(rows[1], ".........d");
    assert_eq!(cursor, (1, 8));
    let (rows, _) = after(&format!("\x1b[?7l{RLM}\x1b[1;2Habc"));
    assert_eq!(rows[0], "ca........");
}

/// **A wide character takes the cursor's column and the one to its left**,
/// its lead on the left, and the cursor moves two; a combining mark joins
/// the character the last write put down.
#[test]
fn a_wide_character_and_a_mark_go_where_the_last_write_was() {
    let (rows, cursor) = after(&format!("{RLM}\x1b[1;10H\u{65E5}"));
    assert_eq!(rows[0], "........\u{65E5}~");
    assert_eq!(cursor, (0, 7));
    support::with_term(&WS, |t| {
        support::feed(t, format!("{RLM}\x1b[1;10He\u{301}x").as_bytes());
        let row = t.viewport().row(Row::ZERO);
        assert!(row[9].flags.is_marked(), "the mark on e");
        assert!(!row[8].flags.is_marked());
        assert_eq!(t.cluster_marks(row[9]).collect::<Vec<_>>(), [0x301]);
    });
}

/// **Under IRM** "All characters from the current cursor position to the
/// left margin inclusively are shifted one position towards the left. The
/// character previously displayed at the left margin disappears."
#[test]
fn under_irm_the_line_shifts_toward_the_left_margin() {
    let (rows, _) = after(&format!("xyz{RLM}\x1b[4h\x1b[1;3HQ"));
    assert_eq!(rows[0], "yzQ.......");
}

/// **`BS` moves right, `CR` to the right margin, `LF` under LNM and `NEL`
/// to the right margin of the next line, `HT` to the previous tab stop**;
/// `CUF` is not affected.
#[test]
fn bs_cr_lnm_nel_and_ht_follow_the_direction() {
    assert_eq!(after(&format!("{RLM}\x1b[1;5H\x08")).1, (0, 5));
    assert_eq!(after(&format!("{RLM}\x1b[1;5H\r")).1, (0, 9));
    assert_eq!(after(&format!("\x1b[20h{RLM}\x1b[1;3H\n")).1, (1, 9));
    assert_eq!(after(&format!("{RLM}\x1b[1;3H\x1bE")).1, (1, 9));
    assert_eq!(after(&format!("{RLM}\x1b[1;10H\t")).1, (0, 8));
    assert_eq!(after(&format!("{RLM}\x1b[1;3H\x1b[C")).1, (0, 3));
}

/// **Inside DECLRMM's margins `CR` goes to the right margin, and the wrap
/// from the left margin lands on the right margin of the next line**
/// (Jev's lean, MARGINS).
#[test]
fn inside_side_margins_the_margins_bound_it() {
    let m = "\x1b[?69h\x1b[3;7s";
    assert_eq!(after(&format!("{m}{RLM}\x1b[1;5H\r")).1, (0, 6));
    let (rows, cursor) = after(&format!("{m}{RLM}\x1b[1;4Habc"));
    assert_eq!(rows[0], "..ba......");
    assert_eq!(rows[1], "......c...");
    assert_eq!(cursor, (1, 5));
}

/// **Reset with a wrap pending at the left margin, the cursor is on that
/// column**: the next character overwrites it, left to right again.
#[test]
fn reset_with_a_wrap_pending_writes_on_from_the_margin() {
    let (rows, cursor) = after(&format!("{RLM}\x1b[1;1Ha\x1b[?34lb"));
    assert_eq!(rows[0], "b.........");
    assert_eq!(cursor, (0, 1));
}

/// **XTSAVE keeps it**, as it keeps every mode DECSET acts on.
#[test]
fn xtsave_keeps_decrlm() {
    assert_eq!(
        decrqm(&format!("{RLM}\x1b[?34s\x1b[?34l\x1b[?34r"), 34),
        "\x1b[?34;1$y"
    );
}

/// **A wrap waiting at the left margin is the same whatever the reads**:
/// the property suite's shrunk case, a double-width status line written
/// under DECRLM, read a byte at a time and whole. The wait first rode in
/// the cursor's column as a tag, which a read's end, keeping the cursor on
/// the line's half, took for a column past the line.
#[test]
fn a_wrap_waiting_at_the_left_edge_does_not_depend_on_the_reads() {
    let p: &[u8] = b"\x1b[2$~\x1b[1$}\x1b[?34h\x1b#6a\xd8\x80\xe2\x82\x1b[1$w";
    let line = |chunk: usize| {
        support::with_term(&WS, |t| {
            for c in p.chunks(chunk) {
                support::feed(t, c);
            }
            let s = t.status_line().expect("a status line");
            let cells: Vec<_> = s.cells.iter().map(|c| c.content).collect();
            (cells, s.cursor)
        })
    };
    assert_eq!(line(1), line(4096));
}

/// **Under Unicode 8's widths too, a combining mark joins the character
/// the last write put down**: written at column 1, `e` lands on column 1
/// and the cursor on column 0, where every line under DECRLM ends; the
/// mark is `e`'s, not a cell of its own. The control is the same bytes by
/// the current widths.
#[test]
fn under_unicode_8_a_mark_at_column_0_joins_the_last_write() {
    let mark_on_e = |prefix: &str| {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{prefix}{RLM}\x1b[1;2He\u{301}").as_bytes());
            let row = t.viewport().row(Row::ZERO);
            (
                row[1].flags.is_marked(),
                t.cluster_marks(row[1]).collect::<Vec<_>>(),
                row[0].content.is_empty(),
            )
        })
    };
    assert_eq!(mark_on_e(""), (true, vec![0x301], true), "the control");
    assert_eq!(
        mark_on_e("\x1b]1337;UnicodeVersion=8\x07"),
        (true, vec![0x301], true)
    );
}

/// **A row erased whole is no longer one DECRLM wrote** (Jev, AS_STORED:
/// drawn as its cells hold, "until it is erased whole"): `ED 2`, `EL 2`,
/// `EL 0` from column one and `DECSED 2` with nothing protected end the
/// mark, and a line written in order on it is drawn by UAX #9 again. An
/// erase that leaves part of the row, `EL 0` from column five or `DECSED`
/// sparing a protected character, keeps it. The line's path, SCP's,
/// survives every erase, as it did.
#[test]
fn a_row_erased_whole_is_no_longer_one_decrlm_wrote() {
    let written = |scp: &str, erase: &str| {
        support::with_term(&WS, |t| {
            support::feed(
                t,
                format!("{scp}{RLM}\x1b[1;8Hab\x1b[?34l{erase}").as_bytes(),
            );
            let line = t.view(Rows::ZERO).bidi(Row::ZERO);
            (line.written(), line.path())
        })
    };
    use baud_domain::CharacterPath::{Default as NoPath, RightToLeft as Rtl};
    for (scp, path) in [("", NoPath), ("\x1b[2 k", Rtl)] {
        assert_eq!(
            written(scp, ""),
            (RowWritten::RightToLeft, path),
            "the control"
        );
        for erase in ["\x1b[2J", "\x1b[H\x1b[2K", "\x1b[H\x1b[K", "\x1b[?2J"] {
            assert_eq!(
                written(scp, erase),
                (RowWritten::InOrder, path),
                "{scp:?} {erase:?}"
            );
        }
        for erase in ["\x1b[1;5H\x1b[K", "\x1b[1;1H\x1b[1\"q*\x1b[0\"q\x1b[?2J"] {
            assert_eq!(
                written(scp, erase),
                (RowWritten::RightToLeft, path),
                "{scp:?} {erase:?}"
            );
        }
    }
}

/// **A row whose cells reach past a narrower screen's edge, erased whole,
/// is no longer one DECRLM wrote**: the erase that runs on to the row's
/// mark past the edge ends the mark too.
#[test]
fn a_row_erased_whole_past_the_edge_is_no_longer_one_decrlm_wrote() {
    let at = |t: &mut Term<'_>, c: u16| {
        t.resize(&WinSize {
            ws_col: c,
            ws_row: 50,
        })
    };
    support::with_room(60, |t| {
        assert_eq!(at(t, 60), Resized::Ok);
        support::feed(
            t,
            format!("\x1b[?1049h{RLM}\x1b[1;40Hab\x1b[?34l").as_bytes(),
        );
        assert_eq!(at(t, 30), Resized::Ok, "narrower");
        let written = |t: &Term<'_>| t.view(Rows::ZERO).bidi(Row::ZERO).written();
        assert_eq!(written(t), RowWritten::RightToLeft, "the control");
        support::feed(t, b"\x1b[2J");
        assert_eq!(written(t), RowWritten::InOrder);
    });
}

/// **A line a scroll brings in is not one DECRLM wrote**, though the ring
/// row it takes once held one: on the primary, on the alternate screen,
/// and on an alternate screen narrowed below the row's cells, where the
/// blank runs on past the edge.
#[test]
fn a_line_scrolled_in_is_not_one_decrlm_wrote() {
    let rows_written = |t: &Term<'_>| {
        let vp = t.view(Rows::ZERO);
        (0..vp.rows().index())
            .filter(|&r| vp.bidi(Row::from_index(r)).written() == RowWritten::RightToLeft)
            .count()
    };
    for screen in ["", "\x1b[?1049h"] {
        support::with_term(&WS, |t| {
            support::feed(
                t,
                format!("{screen}{RLM}\x1b[1;8Hab\x1b[?34l\x1b[4;1H").as_bytes(),
            );
            assert_eq!(rows_written(t), 1, "the control");
            for n in 0..200 {
                support::feed(t, b"\n");
                assert_eq!(rows_written(t), 0, "{screen:?} line feed {n}");
            }
        });
    }
    support::with_room(60, |t| {
        let at = |t: &mut Term<'_>, c: u16| {
            t.resize(&WinSize {
                ws_col: c,
                ws_row: 50,
            })
        };
        assert_eq!(at(t, 60), Resized::Ok);
        support::feed(
            t,
            format!("\x1b[?1049h{RLM}\x1b[1;40Hab\x1b[?34l").as_bytes(),
        );
        assert_eq!(at(t, 30), Resized::Ok, "narrower");
        support::feed(t, b"\x1b[50;1H");
        assert_eq!(rows_written(t), 1, "the control");
        for n in 0..120 {
            support::feed(t, b"\n");
            assert_eq!(rows_written(t), 0, "line feed {n}");
        }
    });
}

/// **`IL` and `DL` bring in lines DECRLM never wrote**, and the line it
/// wrote moves with its mark: `IL` at the line it wrote, `DL` above the
/// line it wrote at the bottom.
#[test]
fn il_and_dl_bring_in_lines_decrlm_never_wrote() {
    let marks = |bytes: String| {
        support::with_term(&WS, |t| {
            support::feed(t, bytes.as_bytes());
            let vp = t.view(Rows::ZERO);
            (0..4)
                .map(|r| vp.bidi(Row::from_index(r)).written() == RowWritten::RightToLeft)
                .collect::<Vec<_>>()
        })
    };
    let il = marks(format!("{RLM}\x1b[1;8Hab\x1b[?34l\x1b[1;1H\x1b[L"));
    assert_eq!(il, [false, true, false, false]);
    let dl = marks(format!("{RLM}\x1b[4;8Hab\x1b[?34l\x1b[1;1H\x1b[M"));
    assert_eq!(dl, [false, false, true, false]);
}
