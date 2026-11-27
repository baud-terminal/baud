//! **SLH and SLL, `CSI Pn SP U` and `CSI Pn SP V`** (`[[source: ecma48]]`
//! 8.3.122, 8.3.123): SLH sets "the position to which the active
//! presentation position will be moved by subsequent occurrences of
//! CARRIAGE RETURN (CR), DELETE LINE (DL), INSERT LINE (IL) or NEXT LINE
//! (NEL)"; under DATA, and with BDSM set, which reads DCSM as DATA (7.3.4
//! c), it is also "the position ahead of which no implicit movement ...
//! shall occur", and SLL "the position beyond which no implicit movement
//! ... shall occur". Positions of their own, apart from DEC's margins.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Rows 0 and 1 as text, `.` for an empty position, and the cursor.
fn after(bytes: &[u8]) -> (String, String, (usize, usize)) {
    support::render(&WS, bytes, |t| screen(t))
}

fn screen(t: &Term<'_>) -> (String, String, (usize, usize)) {
    let row = |y: usize| -> String {
        t.viewport().row(Row::from_index(y))[..20]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect()
    };
    let (r, c) = t.cursor();
    (row(0), row(1), (r.index(), c.index()))
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    after(bytes).2
}

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

/// **`CR`, `NEL`, `LF` under LNM, `IL` and `DL` go to the line home**:
/// position 5, column 4.
#[test]
fn slh_takes_cr_nel_il_and_dl_to_the_line_home() {
    assert_eq!(cursor(b"\x1b[5 Uabcdefgh\r"), (0, 4), "CR");
    assert_eq!(cursor(b"\x1b[5 U\x1b[1;10H\x1bE"), (1, 4), "NEL");
    assert_eq!(
        cursor(b"\x1b[20h\x1b[5 U\x1b[1;10H\n"),
        (1, 4),
        "LF under LNM"
    );
    assert_eq!(cursor(b"\x1b[5 U\x1b[2;10H\x1b[L"), (1, 4), "IL");
    assert_eq!(cursor(b"\x1b[5 U\x1b[2;10H\x1b[M"), (1, 4), "DL");
    assert_eq!(
        cursor(b"\x1b[5 U\x1b[1;3H\r"),
        (0, 4),
        "from before the home"
    );
}

/// **`CNL` and `CPL` go to "the first character position"** (8.3.12,
/// 8.3.13), which SLH does not name.
#[test]
fn cnl_and_cpl_keep_the_first_position() {
    assert_eq!(cursor(b"\x1b[5 U\x1b[1;10H\x1b[E"), (1, 0));
    assert_eq!(cursor(b"\x1b[5 U\x1b[3;10H\x1b[F"), (1, 0));
}

/// **A character written at the line limit wraps to the next line's
/// home** under DECAWM: limit 10, home 3.
#[test]
fn sll_wraps_at_the_line_limit_to_the_home() {
    let mut b = b"\x1b[3 U\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(
        after(&b),
        (
            "abcdefghij..........".into(),
            "..kl................".into(),
            (1, 4)
        )
    );
}

/// With DECAWM reset it stays on the limit, each character written there.
#[test]
fn sll_with_autowrap_reset_stays_at_the_limit() {
    let mut b = b"\x1b[?7l\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(after(&b).0, "abcdefghil..........");
    assert_eq!(after(&b).2, (0, 9));
}

/// **The home alone takes the wrap at the right edge**: the line home is
/// "the position ahead of which no implicit movement ... shall occur".
#[test]
fn slh_alone_takes_the_wrap_at_the_edge_to_the_home() {
    let mut b = b"\x1b[5 U".to_vec();
    b.extend_from_slice(&LETTERS[..25]);
    assert_eq!(after(&b).1, "....uvwxy...........");
}

/// **Under PRESENTATION the limit and the wrap's home do nothing**: those
/// clauses are a "device without data component"'s. BDSM reset, DCSM at
/// PRESENTATION: the text runs to the edge and wraps to column 0, and `CR`
/// still goes to the home. Under DATA (`CSI 9 h`) they apply again.
#[test]
fn under_presentation_only_cr_reads_the_home() {
    let mut b = b"\x1b[8l\x1b[3 U\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..22]);
    let (r0, r1, _) = after(&b);
    assert_eq!(r0, "abcdefghijklmnopqrst");
    assert_eq!(r1, "uv..................");
    assert_eq!(cursor(b"\x1b[8l\x1b[3 Uabcdef\r"), (0, 2));
    let mut b = b"\x1b[8l\x1b[9h\x1b[3 U\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(after(&b).1, "..kl................");
}

/// **The inner bound holds** where DEC's margins are set too: a right
/// margin at 8 wraps before a limit at 10, a limit at 10 before a right
/// margin at 15; a left margin at 5 is past a home at 3.
#[test]
fn the_inner_of_the_margins_and_the_bounds_holds() {
    let mut b = b"\x1b[?69h\x1b[1;8s\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..10]);
    assert_eq!(after(&b).0, "abcdefgh............");
    let mut b = b"\x1b[?69h\x1b[1;15s\x1b[10 V".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(after(&b).1, "kl..................");
    assert_eq!(cursor(b"\x1b[?69h\x1b[5;15s\x1b[3 U\x1b[1;10H\r"), (0, 4));
    assert_eq!(cursor(b"\x1b[?69h\x1b[2;15s\x1b[5 U\x1b[1;10H\r"), (0, 4));
}

/// `Pn` has no default value: an omitted or zero one is counted and sets
/// nothing. A position past the line is its last column.
#[test]
fn a_missing_position_is_counted_and_one_past_the_line_is_its_last() {
    support::render(&WS, b"\x1b[ U\x1b[0 Vabc\r", |t| {
        assert_eq!(t.unsupported_param, 2);
        assert_eq!(t.cursor().1, Col::ZERO);
    });
    assert_eq!(cursor(b"\x1b[99 Uabc\r"), (0, 19));
}

/// **`RIS` forgets them, `DECSTR` keeps them**.
#[test]
fn ris_forgets_the_bounds_and_decstr_keeps_them() {
    assert_eq!(cursor(b"\x1b[5 U\x1bcabcdefg\r"), (0, 0));
    assert_eq!(cursor(b"\x1b[5 U\x1b[!pabcdefg\r"), (0, 4));
    let mut b = b"\x1b[10 V\x1b[!p".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(after(&b).1, "kl..................");
}

/// **A resize keeps them**, a position past the narrower line acting as
/// its last column.
#[test]
fn a_resize_keeps_the_bounds() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[15 U");
        support::shrink(
            t,
            &WinSize {
                ws_row: 4,
                ws_col: 10,
            },
        );
        support::feed(t, b"abc\r");
        assert_eq!(t.cursor().1, Col::at_index(9));
        assert_eq!(t.resize(&WS), Resized::Ok);
        support::feed(t, b"abc\r");
        assert_eq!(t.cursor().1, Col::at_index(14));
    });
}

/// **Under DECRLM `CR` keeps DECRLM's right margin**, SLH naming the
/// home of a left-to-right line: `IL` keeps DEC's left margin there
/// too.
#[test]
fn under_decrlm_cr_keeps_the_right_margin() {
    assert_eq!(cursor(b"\x1b[?34h\x1b[5 U\x1b[1;10H\r"), (0, 19));
    assert_eq!(cursor(b"\x1b[?34h\x1b[5 U\x1b[2;10H\x1b[L"), (1, 0));
}

/// **DECRQSS does not answer them**: no DEC or xterm text lists them, so
/// they are answered as SCP is, `DCS 0 $ r ST`.
#[test]
fn decrqss_answers_slh_and_sll_as_invalid() {
    for q in [&b"\x1b[5 U\x1bP$q U\x1b\\"[..], b"\x1b[5 V\x1bP$q V\x1b\\"] {
        support::render(&WS, q, |t| assert_eq!(t.reply(), b"\x1bP0$r\x1b\\"));
    }
}

/// A wide character that does not fit before the limit wraps whole, as at
/// a right margin.
#[test]
fn a_wide_character_too_wide_for_the_limit_wraps() {
    let (r0, r1, _) = after("\x1b[10 Vabcdefghi中".as_bytes());
    assert_eq!(r0, "abcdefghi...........");
    assert_eq!(r1, "中...................");
}

/// The private namespace is no SLH: `CSI ? 5 SP U` is counted.
#[test]
fn slh_with_a_private_marker_is_no_control() {
    support::render(&WS, b"\x1b[?5 Uabc\r", |t| {
        assert_eq!(t.unsupported, 1);
        assert_eq!(t.cursor().1, Col::ZERO);
    });
}

/// **A wrap pending at the line limit is reported on the limit**: the
/// character written there leaves the active position on it, "the
/// position beyond which no implicit movement of the active data position
/// shall occur" (8.3.123), as DEC's Last Column Flag leaves it on the
/// last column, "the Active Position cannot be advanced" (`[[source:
/// dec-std-070]]` D.6.1), and CPR reports the active position (8.3.14;
/// 070's CPR algorithm, p. 5-54). Limit 5, column 4: `CPR`, DECXCPR,
/// DECCIR, [`Term::cursor`], [`Term::wrap_pending`], HPB, DECBI, DECFI,
/// SPA and EPA, the indicator and a status line's cursor; after a line
/// feed, across which baud keeps the wait as it does at the right margin,
/// where D.6.1 lists LINE FEED, ECH, EL and RM(DECAWM) among the controls
/// that reset the Last Column Flag ("existing products vary widely"), the
/// position the same either way; and after DECSWL and DECAWM reset, which
/// end it.
#[test]
fn a_wrap_pending_at_the_line_limit_is_reported_on_the_limit() {
    let pending = b"\x1b[5 Vabcde";
    support::render(&WS, &[&pending[..], b"\x1b[6n\x1b[?6n"].concat(), |t| {
        assert_eq!(t.reply(), b"\x1b[1;5R\x1b[?1;5;1R");
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(4)));
        assert_eq!(t.wrap_pending(), WrapPending::Yes);
    });
    support::render(&WS, &[&pending[..], b"\n\x1b[6n"].concat(), |t| {
        assert_eq!(t.reply(), b"\x1b[2;5R");
    });
    // DECAWM reset ends the wrap and leaves the position on the limit.
    support::render(&WS, &[&pending[..], b"\x1b[?7l\x1b[6n"].concat(), |t| {
        assert_eq!(t.reply(), b"\x1b[1;5R");
        assert_eq!(t.wrap_pending(), WrapPending::No);
    });
    // The wrap it reports: the next character goes to the next line.
    assert_eq!(
        after(&[&pending[..], b"f"].concat()).1,
        "f..................."
    );
    // DECCIR names the limit, with Sflag's bit 4, "auto-wrap pending".
    support::render(&WS, &[&pending[..], b"\x1b[1$w"].concat(), |t| {
        let r = t.reply().to_vec();
        let want = b"\x1bP1$u1;5;1;@;@;H;";
        assert!(r.starts_with(want), "{:?}", String::from_utf8_lossy(&r));
    });
    // HPB 1 and DECBI move one position back from the limit, DECFI one on.
    for (step, want) in [
        (&b"\x1b[j"[..], &b"\x1b[1;4R"[..]),
        (b"\x1b6", b"\x1b[1;4R"),
        (b"\x1b9", b"\x1b[1;6R"),
    ] {
        support::render(&WS, &[&pending[..], step, b"\x1b[6n"].concat(), |t| {
            assert_eq!(t.reply(), want, "{:?}", String::from_utf8_lossy(step));
        });
    }
    // DECSWL, one of the controls that reset the Last Column Flag (D.6.1),
    // ends the wait on the limit.
    support::render(&WS, &[&pending[..], b"\x1b#5\x1b[6n"].concat(), |t| {
        assert_eq!(t.reply(), b"\x1b[1;5R");
        assert_eq!(t.wrap_pending(), WrapPending::No);
    });
    // SPA and EPA guard the limit, the active position.
    support::render(&WS, &[&pending[..], b"\x1bV\x1bW"].concat(), |t| {
        let row = t.viewport().row(Row::ZERO)[..20]
            .iter()
            .map(|c| if c.flags.is_guarded() { 'G' } else { '.' })
            .collect::<String>();
        assert_eq!(row, "....G...............");
    });
    // The indicator shows it, drawn by the read and again between reads.
    let shown = |t: &Term<'_>| -> String {
        let line = t.status_line().expect("an indicator");
        line.cells
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or(' ')
            })
            .collect()
    };
    support::render(&WS, &[b"\x1b[1$~", &pending[..]].concat(), |t| {
        assert!(shown(t).contains("1(01,005)"), "{:?}", shown(t));
        t.set_printer(PrinterPort::Ready);
        assert!(shown(t).contains("1(01,005)"), "{:?}", shown(t));
    });
    // A host-writable status line's cursor, parked between reads, is
    // drawn on the limit as CPR names it.
    support::render(
        &WS,
        &[b"\x1b[2$~\x1b[1$}", &pending[..], b"\x1b[6n"].concat(),
        |t| {
            assert_eq!(t.reply(), b"\x1b[1;5R");
            let line = t.status_line().expect("a status line");
            assert_eq!(line.cursor, Some(Col::at_index(4)));
        },
    );
}

/// **A cursor a move put past the limit is reported where it is**: `CUP`
/// and `CHA` to position 6, past limit 5, and `CUF 2` to position 6 after
/// `CUB` ended a wrap waiting at the limit, which counts from the limit as
/// it counts from a right margin and leaves the cursor on position 4. The
/// limit bounds implicit movement only, so the active position is the one
/// the move named.
#[test]
fn a_cursor_moved_past_the_line_limit_is_reported_where_it_is() {
    for (bytes, want) in [
        (&b"\x1b[5 V\x1b[1;6H\x1b[6n"[..], &b"\x1b[1;6R"[..]),
        (b"\x1b[5 V\x1b[6G\x1b[6n", b"\x1b[1;6R"),
        (b"\x1b[5 Vabcde\x1b[D\x1b[2C\x1b[6n", b"\x1b[1;6R"),
    ] {
        support::render(&WS, bytes, |t| {
            assert_eq!(t.reply(), want, "{:?}", String::from_utf8_lossy(bytes));
            assert_eq!(t.wrap_pending(), WrapPending::No);
        });
    }
}

/// What `CPR` answers after `bytes`, and whether a wrap still waits.
fn reported(bytes: &[u8]) -> (String, WrapPending) {
    let b = [bytes, b"\x1b[6n"].concat();
    support::render(&WS, &b, |t| {
        (
            String::from_utf8_lossy(t.reply()).into_owned(),
            t.wrap_pending(),
        )
    })
}

/// Every case of `cases` whose report is not the one it names, each named.
fn wrong(cases: &[(&[u8], &str, WrapPending)]) -> Vec<String> {
    cases
        .iter()
        .filter(|(b, want, w)| reported(b) != ((*want).into(), *w))
        .map(|(b, want, _)| {
            let got = reported(b).0;
            format!("{:?}: {:?}, not {want:?}", String::from_utf8_lossy(b), got)
        })
        .collect()
}

/// **A cursor movement from a wrap waiting at the line limit counts from
/// the limit**, the active position (8.3.123 bounds only implicit
/// movement; `[[source: dec-std-070]]` D.6.1, "the Active Position cannot
/// be advanced", its CUF and HT algorithms adding to the active column):
/// limit 5, a stop set at 6, then `CUF`, `CUF 2`, `HPR`, `HT` and `CHT`
/// from the wait; `CBT` with a stop at the limit goes to the stop before
/// it; `CUF` under DECLRMM and under DCSM's DATA with BDSM reset and a
/// string begun, the paths those take, and with a string on the line, whose
/// map the move reads the cursor through, `CHT` and `CBT` too; and `HTS` and `TBC 0` set and clear
/// the stop at the limit, which an `HT` from the line's start then finds.
/// `CUB` counts from the limit too, as it counts from a right margin's
/// pending wrap.
#[test]
fn a_move_from_a_wrap_waiting_at_the_line_limit_counts_from_the_limit() {
    const W: &[u8] = b"\x1b[1;6H\x1bH\x1b[1;1H\x1b[5 Vabcde";
    let no = WrapPending::No;
    let cases: &[(&[u8], &str, WrapPending)] = &[
        (&[W, b"\x1b[C"].concat(), "\x1b[1;6R", no),
        (&[W, b"\x1b[2C"].concat(), "\x1b[1;7R", no),
        (&[W, b"\x1b[a"].concat(), "\x1b[1;6R", no),
        (&[W, b"\t"].concat(), "\x1b[1;6R", no),
        (&[W, b"\x1b[I"].concat(), "\x1b[1;6R", no),
        (
            b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;1H\x1b[5 Vabcde\x1b[Z",
            "\x1b[1;1R",
            no,
        ),
        (b"\x1b[?69h\x1b[1;15s\x1b[5 Vabcde\x1b[C", "\x1b[1;6R", no),
        (
            b"\x1b[8l\x1b[9h\x1b[1]\x1b[0]\x1b[5 Vabcde\x1b[C",
            "\x1b[1;6R",
            no,
        ),
        (
            b"\x1b[8l\x1b[9h\x1b[1]ab\x1b[0]\x1b[5 Vcde\x1b[C",
            "\x1b[1;6R",
            no,
        ),
        (
            b"\x1b[1;6H\x1bH\x1b[1;1H\x1b[8l\x1b[9h\x1b[1]ab\x1b[0]\x1b[5 Vcde\x1b[I",
            "\x1b[1;6R",
            no,
        ),
        (
            b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;1H\x1b[8l\x1b[9h\x1b[1]ab\x1b[0]\x1b[5 Vcde\x1b[Z",
            "\x1b[1;1R",
            no,
        ),
        (b"\x1b[3g\x1b[5 Vabcde\x1bH\r\t", "\x1b[1;5R", no),
        (
            b"\x1b[1;5H\x1bH\x1b[1;1H\x1b[5 Vabcde\x1b[g\r\t",
            "\x1b[1;9R",
            no,
        ),
        (&[W, b"\x1b[D"].concat(), "\x1b[1;4R", no),
    ];
    let bad = wrong(cases);
    assert!(
        bad.is_empty(),
        "{} of {}:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// **`HPR` from a wrap waiting at a DECSLRM right margin counts from the
/// margin**, as `HPB` does: margins 1 to 10, ten letters, `HPR` to
/// position 11, the column one past the margin, where xterm's cursor column
/// is still on the margin while the wrap waits.
#[test]
fn hpr_from_a_wrap_waiting_at_the_right_margin_counts_from_the_margin() {
    let b = b"\x1b[?69h\x1b[1;10sabcdefghij\x1b[a";
    assert_eq!(reported(b), ("\x1b[1;11R".into(), WrapPending::No));
}

/// **An SLL that moves the limit a wrap waits at ends the wait on the
/// position it waited on**, as DECSWL does (`[[source: dec-std-070]]`
/// D.6.1 lists it among the controls that reset the Last Column Flag): to
/// the right and to the left of it, and BDSM reset, under which the limit
/// bounds nothing; moved back, the limit finds no wait left marked; the
/// next character goes on that position. An SLL that names the same
/// position keeps the wait.
#[test]
fn an_sll_that_moves_the_limit_ends_a_wrap_waiting_there() {
    const W: &[u8] = b"\x1b[5 Vabcde";
    const BACK: &[u8] = b"\x1b[5 V\x1b[8h";
    let no = WrapPending::No;
    let cases: &[(&[u8], &str, WrapPending)] = &[
        (&[W, b"\x1b[10 V"].concat(), "\x1b[1;5R", no),
        (&[W, b"\x1b[3 V"].concat(), "\x1b[1;5R", no),
        (&[W, b"\x1b[8l"].concat(), "\x1b[1;5R", no),
        (&[W, b"\x1b[10 V", BACK].concat(), "\x1b[1;5R", no),
        (&[W, b"\x1b[3 V", BACK].concat(), "\x1b[1;5R", no),
        (&[W, b"\x1b[8l", BACK].concat(), "\x1b[1;5R", no),
        // The control: the same position.
        (&[W, b"\x1b[5 V"].concat(), "\x1b[1;5R", WrapPending::Yes),
    ];
    let bad = wrong(cases);
    assert!(
        bad.is_empty(),
        "{} of {}:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
    assert_eq!(after(b"\x1b[5 Vabcde\x1b[10 Vf").0, "abcdf...............");
}

/// **A cursor past the line limit on a double-width line stays inside the
/// line's half, not the limit's**: the limit bounds only implicit
/// movement. DECDWL with the cursor at 8, past limit 5, and `CUD` onto a
/// double line at 8, keep it at 8, where both put it back on the limit;
/// and text written from there runs on to the half's last position, 10,
/// and wraps, where it ran on to the screen's last column, 20.
#[test]
fn past_the_limit_on_a_double_line_the_half_bounds_the_cursor() {
    assert_eq!(
        reported(b"\x1b[5 V\x1b[1;8H\x1b#6"),
        ("\x1b[1;8R".into(), WrapPending::No)
    );
    assert_eq!(
        reported(b"\x1b[2;1H\x1b#6\x1b[5 V\x1b[1;8H\x1b[B"),
        ("\x1b[2;8R".into(), WrapPending::No)
    );
    let (r0, r1, _) = after(b"\x1b#6\x1b[5 V\x1b[1;8Habcdef");
    assert_eq!(r0, ".......abc..........");
    assert_eq!(r1, "def.................");
}

/// **With DECAWM reset, a write past the limit on a double line stops on
/// the half's last column**, "the last column of the Active Line"
/// (`[[source: dec-std-070]]` p. 5-139): from 8, past limit 5, `abc`
/// leaves the cursor on 10, so `CUB 2` goes to 8 and `ICH` makes room on
/// 10, where the cursor went to 11, the half's sentinel, `CUB 2` to 9 and
/// `ICH` made room nowhere; and `U+4E00` from 10 leaves it on 10, where it
/// went to 12, past the sentinel, which `CPR` named.
#[test]
fn without_autowrap_past_the_limit_a_double_line_s_half_pins_the_cursor() {
    let off = b"\x1b[?7l\x1b#6\x1b[5 V";
    let row = |b: &[u8]| after(&[&off[..], b].concat()).0;
    assert_eq!(row(b"\x1b[1;8Habc\x1b[2Dx"), ".......xbc..........");
    assert_eq!(row(b"\x1b[1;8Habc\x1b[@"), ".......ab...........");
    assert_eq!(
        reported(&[&off[..], "\x1b[1;10H\u{4e00}".as_bytes()].concat()),
        ("\x1b[1;10R".into(), WrapPending::No)
    );
}

/// **A line home past a double line's half is its last column**, as one
/// past the screen is the screen's: `CR` under SLH 15 went to 15, in the
/// hidden half, and text was written there. Now it goes to 10, and `ab`
/// writes `a` there and wraps `b` to the next line's home; `IL`, `DL` and
/// a wrap onto a double line go to its 10 too, and so do `NEL` and `LF`
/// under LNM on a double last line below the region, where the feed does
/// not move; `NEL` from the line above, which reached 10 by the move onto
/// the line, is the control.
#[test]
fn a_line_home_past_a_double_line_s_half_is_its_last_column() {
    assert_eq!(cursor(b"\x1b#6\x1b[15 U\x1b[1;3H\r"), (0, 9));
    let (r0, r1, _) = after(b"\x1b#6\x1b[15 U\x1b[1;3H\rab");
    assert_eq!(r0, ".........a..........");
    assert_eq!(r1, "..............b.....");
    let on = b"\x1b[2;1H\x1b#6\x1b[15 U";
    for (op, b) in [
        ("NEL", &b"\x1b[1;3H\x1bE"[..]),
        ("IL", b"\x1b[2;3H\x1b[L"),
        ("DL", b"\x1b[2;3H\x1b[M"),
    ] {
        assert_eq!(cursor(&[&on[..], b].concat()), (1, 9), "{op}");
    }
    let (_, r1, _) = after(&[&on[..], b"\x1b[1;1Habcdefghijklmnopqrstu"].concat());
    assert_eq!(r1, ".........u..........", "a wrap");
    let below = b"\x1b[1;3r\x1b[4;1H\x1b#6\x1b[15 U\x1b[4;3H";
    for (op, b) in [("NEL", &b"\x1bE"[..]), ("LF under LNM", b"\x1b[20h\n")] {
        assert_eq!(cursor(&[&below[..], b].concat()), (3, 9), "{op} below");
    }
}

/// **`NEL` and `LF` under LNM take the home on the line they arrive at**,
/// "the line home position of the following line" (`[[source: ecma48]]`
/// 8.3.86): from a double line under SLH 15 onto a single line they go to
/// 15, where the half's 10 was carried down and `z` was written on 10;
/// onto another double line, to its 10; and from a double line at the
/// bottom of the screen or of a region, onto the single line the scroll
/// brings in, to 15.
#[test]
fn nel_and_lf_under_lnm_take_the_home_on_the_line_they_arrive_at() {
    let dwl = b"\x1b#6\x1b[15 U\x1b[1;3H";
    for (op, b) in [("NEL", &b"\x1bE"[..]), ("LF under LNM", b"\x1b[20h\n")] {
        let (_, r1, at) = after(&[&dwl[..], b, b"z"].concat());
        assert_eq!(r1, "..............z.....", "{op}");
        assert_eq!(at, (1, 15), "{op}");
        let both = b"\x1b[2;1H\x1b#6\x1b[15 U\x1b[1;1H\x1b#6\x1b[1;3H";
        assert_eq!(cursor(&[&both[..], b].concat()), (1, 9), "{op}, double");
        for (at, y, scroll) in [
            ("screen", 3, &b"\x1b[4;1H\x1b#6\x1b[15 U\x1b[4;3H"[..]),
            ("region", 2, b"\x1b[1;3r\x1b[3;1H\x1b#6\x1b[15 U\x1b[3;3H"),
        ] {
            assert_eq!(cursor(&[scroll, b].concat()), (y, 14), "{op}, {at} scroll");
        }
    }
}

/// The host-writable status line as text, `.` for an empty position.
fn status_text(t: &Term<'_>) -> String {
    let line = t.status_line().expect("a status line");
    line.cells[..20]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('.')
        })
        .collect()
}

/// **Each display keeps its own line home and limit; BDSM and DCSM are
/// both displays'.** The host-writable status line "contains its own ANSI
/// text state and settings" (`[[source: dec-std-070]]` 070-14 §14.2, p.
/// 14-4), and no DEC text names SLH or SLL, so a home or a limit set
/// while one display is written is that display's, parked with its
/// margins: limit 5 set on the status line leaves the main display's text
/// running on, one set on the main display leaves the status line's, and
/// a home set on either leaves the other's `CR` at its first column.
/// BDSM and DCSM stay the terminal's, as the status line is laid out
/// under them, so BDSM set on the status line makes the main display's
/// limit bound its text, and BDSM reset there ends a wrap the main
/// display left waiting at its limit, as it ends one on the display
/// written; and BDSM sent on the status line leaves the main display's
/// home where its `CR` goes. `DECSTR` keeps the status line's home, as
/// it keeps the main display's. The main display kept the bound it was left
/// with, and the two displays had one home and one limit.
///
/// **And under side margins no fence the limit no longer gives is left
/// behind**: limit 12, margins 4 to 15, the status line selected and
/// BDSM reset there, which a debug build caught with the main display
/// still fenced at the old limit.
#[test]
fn each_display_keeps_its_own_line_home_and_limit() {
    const ON: &[u8] = b"\x1b[2$~\x1b[1$}";
    const MAIN: &[u8] = b"\x1b[0$}";
    let text = b"abcdefghijkl";
    let mut bad = Vec::new();
    let mut check = |what: &str, got: String, want: &str| {
        if got != want {
            bad.push(format!("{what}: {got:?}, not {want:?}"));
        }
    };
    let rows = |b: &[u8]| {
        let (r0, r1, _) = after(b);
        format!("{r0}/{r1}")
    };
    let at = |b: &[u8]| format!("{:?}", cursor(b));
    let status_at = |b: &[u8]| {
        support::render(&WS, b, |t| {
            format!("{:?}", t.status_line().expect("a status line").cursor)
        })
    };
    check(
        "SLL on the status line",
        rows(&[ON, b"\x1b[5 V", MAIN, text].concat()),
        "abcdefghijkl......../....................",
    );
    check(
        "SLL on the main display",
        support::render(&WS, &[b"\x1b[5 V", ON, text].concat(), |t| status_text(t)),
        "abcdefghijkl........",
    );
    check(
        "SLH on the status line, CR on the main display",
        at(&[ON, b"\x1b[5 U", MAIN, b"abcdefgh\r"].concat()),
        "(0, 0)",
    );
    check(
        "SLH on the main display, CR on the status line",
        status_at(&[b"\x1b[5 U", ON, b"abcdefgh\r"].concat()),
        "Some(Col(0))",
    );
    check(
        "SLH on the status line, kept by DECSTR",
        status_at(&[ON, b"\x1b[5 U\x1b[!p\x1b[1$}abcdefgh\r"].concat()),
        "Some(Col(4))",
    );
    check(
        "SLH on the main display, BDSM sent on the status line",
        at(&[b"\x1b[5 U", ON, b"\x1b[8l\x1b[8h", MAIN, b"abcdefgh\r"].concat()),
        "(0, 4)",
    );
    check(
        "BDSM set on the status line",
        rows(&[b"\x1b[8l\x1b[5 V", ON, b"\x1b[8h", MAIN, text].concat()),
        "abcde.............../fghij...............",
    );
    let ended = [&b"\x1b[5 Vabcde"[..], ON, b"\x1b[8l", MAIN].concat();
    check(
        "a wait at the limit, BDSM reset on the status line",
        support::render(&WS, &[&ended[..], b"\x1b[6n"].concat(), |t| {
            format!(
                "{:?} {:?}",
                String::from_utf8_lossy(t.reply()),
                t.wrap_pending()
            )
        }),
        "\"\\u{1b}[1;5R\" No",
    );
    check(
        "and the next character",
        after(&[&ended[..], b"f"].concat()).0,
        "abcdf...............",
    );
    let margins = b"\x1b[12 V\x1b[3;12r\x1b[?69h\x1b[4;15s";
    check(
        "under side margins",
        after(&[&margins[..], ON, b"\x1b[8l", MAIN, b"abcdefghijklmnopq"].concat()).0,
        "abcdefghijklmno.....",
    );
    check(
        "under side margins, the read ending on the status line",
        rows(&[&margins[..], ON, b"\x1b[8l"].concat()),
        "..................../....................",
    );
    assert!(bad.is_empty(), "{} wrong:\n{}", bad.len(), bad.join("\n"));
}
