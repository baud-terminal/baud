//! **SLH and SLL, `CSI Pn SP U` and `CSI Pn SP V`** (`[[source: ecma48]]`
//! 8.3.122, 8.3.123): SLH sets "the position to which the active
//! presentation position will be moved by subsequent occurrences of
//! CARRIAGE RETURN (CR), DELETE LINE (DL), INSERT LINE (IL) or NEXT LINE
//! (NEL)"; under DATA, and with BDSM set, which reads DCSM as DATA (7.3.4
//! c), it is also "the position ahead of which no implicit movement ...
//! shall occur", and SLL "the position beyond which no implicit movement
//! ... shall occur". Positions of their own, apart from DEC's margins
//! (Jev, OWN_POSITIONS, 0.96/0.98).

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
/// nothing. A position past the line is its last column (Jev split,
/// CLAMPED 0.04/0.06).
#[test]
fn a_missing_position_is_counted_and_one_past_the_line_is_its_last() {
    support::render(&WS, b"\x1b[ U\x1b[0 Vabc\r", |t| {
        assert_eq!(t.unsupported_param, 2);
        assert_eq!(t.cursor().1, Col::ZERO);
    });
    assert_eq!(cursor(b"\x1b[99 Uabc\r"), (0, 19));
}

/// **`RIS` forgets them, `DECSTR` keeps them** (Jev, KEEP, 0.96/0.96).
#[test]
fn ris_forgets_the_bounds_and_decstr_keeps_them() {
    assert_eq!(cursor(b"\x1b[5 U\x1bcabcdefg\r"), (0, 0));
    assert_eq!(cursor(b"\x1b[5 U\x1b[!pabcdefg\r"), (0, 4));
    let mut b = b"\x1b[10 V\x1b[!p".to_vec();
    b.extend_from_slice(&LETTERS[..12]);
    assert_eq!(after(&b).1, "kl..................");
}

/// **A resize keeps them**, a position past the narrower line acting as
/// its last column (Jev, KEPT_CLAMPED, 1.00/1.00).
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

/// **Under DECRLM `CR` keeps DECRLM's right margin** (Jev's lean,
/// DECRLM_MARGIN), SLH naming the home of a left-to-right line: `IL`
/// keeps DEC's left margin there too.
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
/// and `CHA` to position 6, past limit 5, and `CUF` to position 6 after
/// `CUB` ended a wrap waiting at the limit, which leaves the cursor on
/// the limit as it leaves one at a right margin on the margin. The limit
/// bounds implicit movement only, so the active position is the one the
/// move named.
#[test]
fn a_cursor_moved_past_the_line_limit_is_reported_where_it_is() {
    for (bytes, want) in [
        (&b"\x1b[5 V\x1b[1;6H\x1b[6n"[..], &b"\x1b[1;6R"[..]),
        (b"\x1b[5 V\x1b[6G\x1b[6n", b"\x1b[1;6R"),
        (b"\x1b[5 Vabcde\x1b[D\x1b[C\x1b[6n", b"\x1b[1;6R"),
    ] {
        support::render(&WS, bytes, |t| {
            assert_eq!(t.reply(), want, "{:?}", String::from_utf8_lossy(bytes));
            assert_eq!(t.wrap_pending(), WrapPending::No);
        });
    }
}
