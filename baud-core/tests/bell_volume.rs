//! DECSWBV and DECSMBV, the warning and margin bells' volumes, `?44`, and
//! the margin bell itself.
//!
//! `[[source: vt520-rm]]` p. 5-154, DECSWBV `CSI Ps SP t`: "1 Off; 2, 3, 4
//! Low; 5, 5, 7, 8, 0 or none High (default)"; p. 5-138, DECSMBV `CSI Ps SP
//! u`: "1, 0 or none Off (default); 2, 3, 4 Low; 5, 6, 7, 8 High". The
//! VT520's DECRQSS list names both (pp. 5-103 and 5-104). `?44` is xterm's
//! margin bell switch (`[[source: ctlseqs]]`). The margin bell is DEC STD
//! 070's (070-6 § 6.6.5.2, `[[source: dec-std-070]]` pp. 6-45 and 6-46): a
//! normal keystroke arms it, and a character printed from a column at or
//! left of RIGHT MARGIN - 8 to one right of it on the same line rings it.
//! Before, both controls were counted and dropped, `?44` was counted as
//! unsupported and DECRQM answered it `0`, and nothing rang a margin bell.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 80,
};

fn ch(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

/// A key typed, its bytes taken from the reply as the embedder takes them.
fn typed(t: &mut Term<'_>, code: KeyCode) {
    assert!(
        t.key(KeyEvent::press(code, Modifiers::NONE)),
        "the key is sent"
    );
    t.clear_reply();
}

/// The DECRQSS answer for `pt` after `bytes`.
fn decrqss(bytes: &[u8], pt: &[u8]) -> Vec<u8> {
    support::with_term(&WS, |t| {
        support::feed(t, bytes);
        t.clear_reply();
        support::feed(t, &[b"\x1bP$q".as_slice(), pt, b"\x1b\\"].concat());
        t.reply().to_vec()
    })
}

/// The volumes, `?44` and the unsupported count after `bytes`, fed at read
/// sizes 1, 2 and whole, which must agree.
fn after(bytes: &[u8]) -> (BellVolume, BellVolume, MarginBell, usize) {
    let mut seen = None;
    for chunk in [1, 2, bytes.len().max(1)] {
        let got = support::with_term(&WS, |t| {
            for piece in bytes.chunks(chunk) {
                support::feed(t, piece);
            }
            (
                t.warning_bell_volume(),
                t.margin_bell_volume(),
                t.margin_bell(),
                t.unsupported,
            )
        });
        assert_eq!(*seen.get_or_insert(got), got, "read size {chunk}");
    }
    seen.expect("one read size")
}

#[test]
fn the_warning_bell_powers_on_high_and_the_margin_bell_off() {
    let (w, m, s, u) = after(b"");
    assert_eq!(
        (w, m, s, u),
        (BellVolume::High, BellVolume::Off, MarginBell::Off, 0)
    );
}

/// **0 and none are DEC's**: DECSWBV high, DECSMBV off -- the reverse of
/// ctlseqs' and xterm's reading of 0 (Jev, DEC, 1.00 and 1.00).
#[test]
fn decswbv_reads_every_value_as_the_dec_manuals_give_it() {
    use BellVolume::*;
    for (ps, want) in [
        ("", High),
        ("0", High),
        ("1", Off),
        ("2", Low),
        ("3", Low),
        ("4", Low),
        ("5", High),
        ("6", High),
        ("7", High),
        ("8", High),
    ] {
        // From low, so that every answer is a change.
        let got = after(format!("\x1b[2 t\x1b[{ps} t").as_bytes());
        assert_eq!(got.0, want, "CSI {ps} SP t");
        assert_eq!(got.3, 0, "CSI {ps} SP t is acted on, not counted");
    }
}

#[test]
fn decsmbv_reads_every_value_as_the_dec_manuals_give_it() {
    use BellVolume::*;
    for (ps, want) in [
        ("", Off),
        ("0", Off),
        ("1", Off),
        ("2", Low),
        ("3", Low),
        ("4", Low),
        ("5", High),
        ("6", High),
        ("7", High),
        ("8", High),
    ] {
        let got = after(format!("\x1b[2 u\x1b[{ps} u").as_bytes());
        assert_eq!(got.1, want, "CSI {ps} SP u");
        assert_eq!(got.3, 0, "CSI {ps} SP u is acted on, not counted");
    }
}

/// A value no text defines changes nothing and is counted.
#[test]
fn a_volume_past_eight_is_refused_and_counted() {
    let (w, m, _, u) = after(b"\x1b[2 t\x1b[3 u\x1b[9 t\x1b[9 u");
    assert_eq!((w, m), (BellVolume::Low, BellVolume::Low));
    assert_eq!(u, 2);
}

/// Only the standard namespace: `CSI ? 1 SP t` is no DECSWBV.
#[test]
fn a_prefixed_form_is_not_a_volume() {
    let (w, m, _, u) = after(b"\x1b[?1 t\x1b[?5 u");
    assert_eq!((w, m), (BellVolume::High, BellVolume::Off));
    assert_eq!(u, 2);
}

/// **DECRQSS answers each volume as the control that sets it**, `1`, `2`
/// or `5`, never the 0 DEC and xterm read apart; sent back, the answer sets
/// the volume it reports.
#[test]
fn decrqss_answers_each_volume_as_the_control_that_sets_it() {
    for (set, pt, want) in [
        (&b""[..], &b" t"[..], &b"\x1bP1$r5 t\x1b\\"[..]),
        (b"\x1b[3 t", b" t", b"\x1bP1$r2 t\x1b\\"),
        (b"\x1b[1 t", b" t", b"\x1bP1$r1 t\x1b\\"),
        (b"", b" u", b"\x1bP1$r1 u\x1b\\"),
        (b"\x1b[4 u", b" u", b"\x1bP1$r2 u\x1b\\"),
        (b"\x1b[8 u", b" u", b"\x1bP1$r5 u\x1b\\"),
    ] {
        let got = decrqss(set, pt);
        assert_eq!(
            got,
            want,
            "{set:?} {pt:?}: {:?}",
            String::from_utf8_lossy(&got)
        );
        // The answer's control, sent back, sets what it reports.
        let control = &want[5..want.len() - 2];
        let mut again = b"\x1b[".to_vec();
        again.extend_from_slice(control);
        assert_eq!(
            decrqss(&[b"\x1b[3 t\x1b[3 u".as_slice(), &again].concat(), pt),
            want
        );
    }
}

/// **RIS restores the power-on volumes and resets `?44`; DECSTR keeps all
/// three**: RIS "replaces all set-up features with their saved settings"
/// (`[[source: vt510-rm-b]]` p. 5-184), and DECSTR's Table 5-9 names
/// neither volume.
#[test]
fn ris_restores_the_volumes_and_44_and_decstr_keeps_them() {
    let set = b"\x1b[1 t\x1b[6 u\x1b[?44h";
    assert_eq!(
        after(&[set.as_slice(), b"\x1b[!p"].concat()),
        (BellVolume::Off, BellVolume::High, MarginBell::On, 0)
    );
    assert_eq!(
        after(&[set.as_slice(), b"\x1bc"].concat()),
        (BellVolume::High, BellVolume::Off, MarginBell::Off, 0)
    );
}

/// `?44` sets and resets alone, DECRQM answers it as xterm's
/// `MdBool(screen->marginbell)`, and XTSAVE and XTRESTORE carry it.
#[test]
fn decrqm_answers_44_and_xtsave_carries_it() {
    let rqm = |bytes: &[u8]| {
        support::with_term(&WS, |t| {
            support::feed(t, bytes);
            t.clear_reply();
            support::feed(t, b"\x1b[?44$p");
            t.reply().to_vec()
        })
    };
    assert_eq!(rqm(b""), b"\x1b[?44;2$y");
    assert_eq!(rqm(b"\x1b[?44h"), b"\x1b[?44;1$y");
    assert_eq!(rqm(b"\x1b[?44h\x1b[?44l"), b"\x1b[?44;2$y");
    assert_eq!(
        rqm(b"\x1b[?44h\x1b[?44s\x1b[?44l\x1b[?44r"),
        b"\x1b[?44;1$y"
    );
    assert_eq!(after(b"\x1b[?44h").3, 0, "acted on, not counted");
}

/// A terminal with the margin bell on, the cursor at column `col` (1-based)
/// of row `row`, before anything is typed.
fn armed_at(t: &mut Term<'_>, row: usize, col: usize) {
    support::feed(t, b"\x1b[?44h\x1b[5 u");
    support::feed(t, format!("\x1b[{row};{col}H").as_bytes());
}

/// The margin bells rung after `steps`: `Key` types, `Host` feeds.
enum Step<'a> {
    Key(char),
    Up,
    Host(&'a [u8]),
}

fn rings(ws: &WinSize, setup: &[u8], steps: &[Step<'_>]) -> usize {
    support::with_term(ws, |t| {
        support::feed(t, setup);
        for s in steps {
            match s {
                Step::Key(c) => typed(t, ch(*c)),
                Step::Up => typed(t, KeyCode::Up),
                Step::Host(b) => support::feed(t, b),
            }
        }
        t.margin_bells
    })
}

const ON: &[u8] = b"\x1b[?44h\x1b[5 u";

fn at(row: usize, col: usize) -> Vec<u8> {
    [ON, format!("\x1b[{row};{col}H").as_bytes()].concat()
}

/// **The 72nd character typed rings it**: printed in column 72, it moves
/// the cursor from 72, which is RIGHT MARGIN - 8, to 73. The 71st does not,
/// and neither does the 73rd, from a column already past it.
#[test]
fn the_character_printed_in_column_72_rings_and_no_other() {
    use Step::*;
    let one = |col: usize| rings(&WS, &at(2, col), &[Key('a'), Host(b"a")]);
    assert_eq!(one(71), 0, "column 71 to 72: not past 72");
    assert_eq!(one(72), 1, "column 72 to 73: past it");
    assert_eq!(one(73), 0, "from 73: never at or left of 72");
    // Armed, the 71st and then the 72nd: one ring, as the cursor crosses.
    assert_eq!(
        rings(
            &WS,
            &at(2, 71),
            &[Key('a'), Host(b"a"), Key('b'), Host(b"b")]
        ),
        1
    );
    // The same keystroke armed it for both: rung once, then disarmed.
    assert_eq!(rings(&WS, &at(2, 71), &[Key('a'), Host(b"ab")]), 1);
    assert_eq!(
        rings(
            &WS,
            &at(2, 72),
            &[Key('a'), Host(b"a"), Host(b"\x1b[2;72Hb")]
        ),
        1,
        "rung, it is disarmed until the next keystroke"
    );
    armed_at_is_used();
}

fn armed_at_is_used() {
    support::with_term(&WS, |t| {
        armed_at(t, 1, 72);
        typed(t, ch('a'));
        support::feed(t, b"a");
        assert_eq!(t.margin_bells, 1);
    });
}

/// **Both switches**: `?44` and a volume not off (Jev's lean,
/// TWO_SWITCHES); either alone rings nothing.
#[test]
fn the_margin_bell_needs_44_and_a_volume() {
    use Step::*;
    let steps = [Key('a'), Host(b"a")];
    assert_eq!(
        rings(&WS, b"\x1b[?44h\x1b[1;72H", &steps),
        0,
        "?44, volume off"
    );
    assert_eq!(
        rings(&WS, b"\x1b[5 u\x1b[1;72H", &steps),
        0,
        "volume, ?44 off"
    );
    assert_eq!(rings(&WS, b"\x1b[?44h\x1b[2 u\x1b[1;72H", &steps), 1, "low");
    assert_eq!(
        rings(&WS, b"\x1b[?44h\x1b[8 u\x1b[1;72H", &steps),
        1,
        "high"
    );
}

/// **Output alone never rings it**: only a normal keystroke arms it.
#[test]
fn output_with_no_keystroke_does_not_ring() {
    assert_eq!(rings(&WS, &at(1, 1), &[Step::Host(&[b'x'; 79])]), 0);
}

/// **A keystroke whose first character is ESC, CSI or DCS arms nothing**
/// (070-6 p. 6-45): a cursor key.
#[test]
fn a_cursor_key_does_not_arm_it() {
    use Step::*;
    assert_eq!(rings(&WS, &at(2, 72), &[Up, Host(b"a")]), 0);
    // 8-bit CSI, under S8C1T: still not a normal keystroke.
    assert_eq!(
        rings(
            &WS,
            &[b"\x1b G".as_slice(), &at(2, 72)].concat(),
            &[Up, Host(b"a")]
        ),
        0
    );
}

/// **"The cursor moves to a new line" disarms it**, STD 070's own example:
/// a keystroke, a linefeed leaving the cursor in the same column, and the
/// echoed character rings nothing. Each way to a new line: LF mid-screen,
/// LF at the bottom (the ring rotates), LF at a region's bottom (rows
/// copied), `IL` and `DL` at the cursor, a screen switch, a scroll on the
/// alternate screen, a wrap.
#[test]
fn every_way_to_a_new_line_disarms_it() {
    use Step::*;
    let cases: [(&str, Vec<u8>, &[u8]); 8] = [
        ("LF mid-screen", at(2, 72), b"\n"),
        ("LF at the bottom", at(6, 72), b"\n"),
        (
            "LF at a region's bottom",
            [b"\x1b[2;4r".as_slice(), &at(4, 72)].concat(),
            b"\n",
        ),
        ("IL", at(2, 72), b"\x1b[L"),
        ("DL", at(2, 72), b"\x1b[M"),
        ("to the alternate screen", at(2, 72), b"\x1b[?47h"),
        (
            "LF at the alternate screen's bottom",
            [b"\x1b[?1049h".as_slice(), &at(6, 72)].concat(),
            b"\n",
        ),
        ("a wrap", at(2, 72), b"\x1b[2;79Hxyz\x1b[2;72H"),
    ];
    for (why, setup, moved) in cases {
        assert_eq!(
            rings(&WS, &setup, &[Key('a'), Host(b"a")]),
            1,
            "{why}: the control"
        );
        assert_eq!(
            rings(&WS, &setup, &[Key('a'), Host(moved), Host(b"a")]),
            0,
            "{why}"
        );
    }
}

/// **Horizontal positioning neither rings nor disarms it** (070-6 p.
/// 6-46): `CUF` across the column rings nothing, and a character printed
/// across it after a `CUF` to its left still rings.
#[test]
fn horizontal_moves_neither_ring_nor_disarm() {
    use Step::*;
    assert_eq!(rings(&WS, &at(2, 60), &[Key('a'), Host(b"\x1b[20C")]), 0);
    assert_eq!(rings(&WS, &at(2, 60), &[Key('a'), Host(b"\x1b[12Ca")]), 1);
    assert_eq!(rings(&WS, &at(2, 72), &[Key('a'), Host(b"\r\x1b[71Ca")]), 1);
}

/// **`?44` reset or the volume off disarms it**, "Margin Bell is disabled".
#[test]
fn turning_it_off_disarms_it() {
    use Step::*;
    for off in [
        &b"\x1b[?44l\x1b[?44h"[..],
        b"\x1b[1 u\x1b[5 u",
        b"\x1bc\x1b[?44h\x1b[5 u\x1b[2;72H",
    ] {
        assert_eq!(
            rings(&WS, &at(2, 72), &[Key('a'), Host(off), Host(b"a")]),
            0,
            "{off:?}"
        );
    }
}

/// **Every writer of text crosses it**: the short-run loop, the ASCII
/// blit, a narrow and a wide run, `REP`, a run that wraps, a character
/// through GR and one through the per-character decode.
#[test]
fn every_writer_rings_it() {
    use Step::*;
    for (why, col, host) in [
        ("one ASCII character", 72, "a".to_string()),
        ("two ASCII characters, one at a time", 71, "ab".to_string()),
        ("a long ASCII run, blitted", 60, "x".repeat(15)),
        ("a narrow run", 70, "\u{e9}\u{e9}\u{e9}\u{e9}".to_string()),
        ("a wide run", 69, "\u{6f22}\u{5b57}\u{6f22}".to_string()),
        ("a wide character from 71", 71, "\u{6f22}".to_string()),
        ("REP", 70, "a\x1b[5b".to_string()),
        ("a run that wraps", 70, "x".repeat(15)),
        ("a three-byte narrow character", 72, "\u{20ac}".to_string()),
        ("a four-byte character", 71, "\u{1f600}".to_string()),
    ] {
        let n = rings(&WS, &at(2, col), &[Key('a'), Host(host.as_bytes())]);
        assert_eq!(n, 1, "{why}");
    }
}

/// **The echo rings it too**: with SRM reset the terminal draws the key
/// itself, after the keystroke armed it.
#[test]
fn a_local_echo_rings_it() {
    let n = rings(
        &WS,
        &[at(2, 72).as_slice(), b"\x1b[12l"].concat(),
        &[Step::Key('a')],
    );
    assert_eq!(n, 1);
}

/// **The right margin is the line's**: on a double-width line the last
/// column is 40 of 80, so it rings from column 32; under side margins the
/// right margin's.
#[test]
fn the_right_margin_is_the_lines() {
    use Step::*;
    let wide = [ON, b"\x1b[2;1H\x1b#6\x1b[2;32H"].concat();
    assert_eq!(
        rings(&WS, &wide, &[Key('a'), Host(b"a")]),
        1,
        "double width, 32"
    );
    let wide = [ON, b"\x1b[2;1H\x1b#6\x1b[2;31H"].concat();
    assert_eq!(
        rings(&WS, &wide, &[Key('a'), Host(b"a")]),
        0,
        "double width, 31"
    );
    let fenced = [ON, b"\x1b[?69h\x1b[1;50s\x1b[2;42H"].concat();
    assert_eq!(
        rings(&WS, &fenced, &[Key('a'), Host(b"a")]),
        1,
        "margins 1-50, 42"
    );
}

/// A screen eight columns wide or less has no column to cross.
#[test]
fn a_narrow_screen_never_rings() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 8,
    };
    for col in 1..=8 {
        let setup = [ON, format!("\x1b[1;{col}H").as_bytes()].concat();
        assert_eq!(
            rings(&ws, &setup, &[Step::Key('a'), Step::Host(b"a")]),
            0,
            "{col}"
        );
    }
}

/// **Where the reads fall changes nothing**: the echo split byte by byte.
#[test]
fn the_ring_lands_the_same_at_every_read_split() {
    for chunk in [1, 2, 3] {
        let n = support::with_term(&WS, |t| {
            support::feed(t, &at(2, 70));
            typed(t, ch('a'));
            for piece in "\u{e9}a\u{6f22}".as_bytes().chunks(chunk) {
                support::feed(t, piece);
            }
            t.margin_bells
        });
        assert_eq!(n, 1, "read size {chunk}");
    }
}

/// **A write that prints no character neither rings it nor disarms it**:
/// STD 070 rings only for "Any character is printed to the screen which
/// causes the cursor to move" (070-6 p. 6-46). DECDWL and DECDHL clear the
/// line's right half, and DECFRA and DECCRA write rectangles, through the
/// same hook a printed character takes; on the armed line they rang it,
/// and on another line they disarmed it.
#[test]
fn a_write_that_prints_nothing_neither_rings_nor_disarms_it() {
    use Step::*;
    let mut bad = Vec::new();
    for (why, op) in [
        ("DECDWL", &b"\x1b#6"[..]),
        ("DECDHL", b"\x1b#3"),
        ("DECFRA on the armed line", b"\x1b[88;2;1;2;80$x"),
        ("DECCRA onto the armed line", b"\x1b[4;1;4;80;1;2;1;1$v"),
    ] {
        let n = rings(&WS, &at(2, 20), &[Key('a'), Host(op)]);
        if n != 0 {
            bad.push(format!("{why}: rang {n}"));
        }
    }
    // On the armed line at 72 the echo after each still rings: the bell
    // stayed armed. DECDWL halves the line, so its echo is from 32.
    for (why, col, op) in [
        ("DECDWL", 32, &b"\x1b#6"[..]),
        ("DECFRA on the armed line", 72, b"\x1b[88;2;1;2;80$x"),
        ("DECFRA on another line", 72, b"\x1b[88;4;1;4;80$x"),
        ("DECCRA onto another line", 72, b"\x1b[4;1;4;80;1;5;1;1$v"),
    ] {
        let echo = format!("\x1b[2;{col}Ha");
        let n = rings(
            &WS,
            &at(2, 72),
            &[Key('a'), Host(op), Host(echo.as_bytes())],
        );
        if n != 1 {
            bad.push(format!("{why}, then the echo from {col}: rang {n}"));
        }
        // In one read, where the hooks are not read afresh between.
        let one = [op, echo.as_bytes()].concat();
        let n = rings(&WS, &at(2, 72), &[Key('a'), Host(&one)]);
        if n != 1 {
            bad.push(format!(
                "{why} and the echo from {col} in one read: rang {n}"
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// **A scroll that does not move the cursor's line keeps it armed**: STD
/// 070 disarms it when "The cursor moves to a new line", and a region
/// scrolled above or below the cursor moves no text on its line. A region
/// in the middle of the screen copies its rows; one at the top rotates the
/// ring and puts the rows under it back, on either screen.
#[test]
fn a_scroll_away_from_the_cursors_line_keeps_it_armed() {
    use Step::*;
    let region = |r: &[u8], row: usize, col: usize| [ON, r, &at(row, col)].concat();
    let fenced = b"\x1b[?69h\x1b[1;50s";
    let cases: [(&str, Vec<u8>, &[u8], usize); 10] = [
        (
            "SU, the region below",
            region(b"\x1b[4;6r", 2, 72),
            b"\x1b[S",
            1,
        ),
        (
            "SD, the region below",
            region(b"\x1b[4;6r", 2, 72),
            b"\x1b[T",
            1,
        ),
        (
            "SU, a top region above",
            region(b"\x1b[1;4r", 6, 72),
            b"\x1b[S",
            1,
        ),
        (
            "SU, a top region above, alternate screen",
            region(b"\x1b[?1049h\x1b[1;4r", 6, 72),
            b"\x1b[S",
            1,
        ),
        (
            "SU, side margins, the region below",
            region(&[&fenced[..], b"\x1b[4;6r"].concat(), 2, 42),
            b"\x1b[S",
            1,
        ),
        // The controls: the cursor's line in the region moves.
        (
            "SU, a top region holding it",
            region(b"\x1b[1;4r", 2, 72),
            b"\x1b[S",
            0,
        ),
        (
            "SU, a region holding it",
            region(b"\x1b[2;4r", 3, 72),
            b"\x1b[S",
            0,
        ),
        (
            "SD, a region holding it",
            region(b"\x1b[4;6r", 5, 72),
            b"\x1b[T",
            0,
        ),
        (
            "SU, side margins, holding it",
            region(&fenced[..], 2, 42),
            b"\x1b[S",
            0,
        ),
        (
            "IL in a region holding it",
            region(b"\x1b[2;4r", 3, 72),
            b"\x1b[L",
            0,
        ),
    ];
    let mut bad = Vec::new();
    for (why, setup, scroll, want) in cases {
        assert_eq!(
            rings(&WS, &setup, &[Key('a'), Host(b"a")]),
            1,
            "{why}: the control"
        );
        let n = rings(&WS, &setup, &[Key('a'), Host(scroll), Host(b"a")]);
        if n != want {
            bad.push(format!("{why}: rang {n}, not {want}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// **Each bell is counted at the volume it rang at**, not at the one the
/// read leaves: DECSWBV around a `BEL` in one read, `RIS` after one, a
/// `BEL` inside Sixel data, at every read split; and the margin bell at
/// DECSMBV's when it rang.
#[test]
fn each_bell_is_counted_at_the_volume_it_rang_at() {
    let rung = |off, low, high| BellsRung { off, low, high };
    for (bytes, want) in [
        (&b"\x1b[1 t\x07\x1b[5 t\x07\x07"[..], rung(1, 0, 2)),
        (b"\x07\x1b[3 t\x07", rung(0, 1, 1)),
        (b"\x1b[1 t\x07\x1bc\x07", rung(1, 0, 1)),
        (b"\x1b[1 t\x07\x1b[!p\x07", rung(2, 0, 0)),
        (b"\x1b[2 t\x1bPq\x07\x1b\\\x1b[5 t", rung(0, 1, 0)),
    ] {
        for chunk in [1, 2, bytes.len()] {
            let (got, bells) = support::with_term(&WS, |t| {
                for piece in bytes.chunks(chunk) {
                    support::feed(t, piece);
                }
                (t.bells_rung(), t.bells)
            });
            assert_eq!(got, want, "{bytes:?} at read size {chunk}");
            assert_eq!(got.off + got.low + got.high, bells, "{bytes:?}");
        }
    }
    let n = support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?44h\x1b[2 u\x1b[2;72H");
        typed(t, ch('a'));
        support::feed(t, b"a\x1b[8 u");
        assert_eq!(t.margin_bells, 1, "the control: it rang");
        t.margin_bells_rung()
    });
    assert_eq!(n, rung(0, 1, 0), "the margin bell at low, as it rang");
    // What a reader asks of two readings.
    use BellVolume::*;
    let before = rung(3, 2, 1);
    for (now, want) in [
        (before, None),
        (rung(4, 2, 1), Some(Off)),
        (rung(4, 3, 1), Some(Low)),
        (rung(3, 2, 2), Some(High)),
        (rung(9, 9, 2), Some(High)),
    ] {
        assert_eq!(now.loudest_since(before), want, "{now:?}");
    }
}
