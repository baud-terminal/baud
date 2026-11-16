//! `?3` DECCOLM behind xterm's `?40`, `?95` DECNCSM, and DECSCPP.
//!
//! `[[source: ctlseqs]]`: "Ps = 3 -> 132 Column Mode (DECCOLM), VT100" and
//! "Ps = 4 0 -> Allow 80 -> 132 mode, xterm"; "Ps = 9 5 -> Do not clear screen
//! when DECCOLM is set/reset (DECNCSM), VT510 and up"; "CSI Ps $ | Select
//! columns per page (DECSCPP), VT340". `[[source: dec-std-070]]` p. 5-71:
//! "Selecting Eighty or One Thirty Two Column Mode causes the display data to
//! be cleared, the scrolling region to be eliminated, and the Active Position
//! to be set to the display origin (1,1), even if the terminal was already in
//! the selected state", and `set_column_mode` (p. 5-72) resets all four
//! margins. `[[source: vt510-decncsm]]`: "When enabled, a column mode change
//! ... does not clear the screen." DECSCPP (p. 5-73) "does not clear page
//! memory or reset the scrolling regions", and `set_columns_per_page`
//! (p. 5-74) reads `Pn <= 80` as 80 and anything else as 132.
//!
//! **The width is asked for, not taken** (`Term::requested_width`): the
//! window is the embedder's. An embedder that resizes sets
//! `WidthHold::On`, and the read stops at the request so the bytes after it
//! land at the width asked for (`honoured`, below, is that embedder).

use baud_core::*;

/// The two pages, as a request reads them.
const ASKED_80: Option<WidthRequest> = Some(WidthRequest::Page(PageWidth::Eighty));
const ASKED_132: Option<WidthRequest> = Some(WidthRequest::Page(PageWidth::OneThirtyTwo));

/// The window a program starts in, and the room its storage has to grow.
const SMALL: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};
const ROOM: WinSize = WinSize {
    ws_row: 24,
    ws_col: 132,
};

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

/// What a test reads back: the screen's text, the cursor, the width, the
/// width last asked for, the `DECRQM` answers for `?3`, `?40`, `?95` and
/// `?69`, the `DECRQSS` answers for both margin pairs, and the refusals.
#[derive(PartialEq, Eq, Debug)]
struct Seen {
    text: Vec<String>,
    cursor: (usize, usize),
    cols: Cols,
    asked: Option<WidthRequest>,
    modes: String,
    margins: String,
    refused: ColumnRefusals,
}

fn seen(t: &mut Term<'_>) -> Seen {
    let text = (0..t.geometry().1.index())
        .map(|y| text_of(t.row(Row::from_index(y)).expect("a row on the grid")))
        .collect();
    let (r, c) = t.cursor();
    let cursor = (r.index(), c.index());
    let cols = t.geometry().0;
    t.clear_reply();
    // The held flag must not stop these: they ask for nothing.
    feed(t, b"\x1b[?3$p\x1b[?40$p\x1b[?95$p\x1b[?69$p");
    let modes = String::from_utf8_lossy(t.reply()).into_owned();
    t.clear_reply();
    feed(t, b"\x1bP$qr\x1b\\\x1bP$qs\x1b\\");
    let margins = String::from_utf8_lossy(t.reply()).into_owned();
    t.clear_reply();
    Seen {
        text,
        cursor,
        cols,
        asked: t.requested_width(),
        modes,
        margins,
        refused: t.column_mode_refusals(),
    }
}

/// `program` in reads of `size` to a terminal that never resizes.
fn unheld(program: &[u8], size: usize) -> Seen {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &SMALL);
    for chunk in program.chunks(size.max(1)) {
        feed(&mut t, chunk);
    }
    seen(&mut t)
}

/// **The embedder that honours a request**, as the macOS app does: the
/// read stops at it, the grid takes the width asked for keeping its rows,
/// by the resize that answers it (`Term::resize_page`), and the rest of the
/// read is fed. Each read is cut at `cuts`.
fn honoured_at(program: &[u8], cuts: &[usize]) -> Seen {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &SMALL);
    honour_into(&mut t, program, cuts);
    seen(&mut t)
}

/// `program` to `t` as [`honoured_at`]'s embedder feeds it, every byte
/// counted once.
fn honour_into(t: &mut Term<'_>, program: &[u8], cuts: &[usize]) {
    t.set_width_hold(WidthHold::On);
    let (before, _) = t.amplification();
    let mut from = 0;
    for &to in cuts.iter().chain(core::iter::once(&program.len())) {
        let chunk = &program[from..to];
        from = to;
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        while let Some(rest) = t.take_held() {
            let rows = t.geometry().1;
            let cols = t.requested_width().expect("a held read asked").ws_col();
            let ws = WinSize {
                ws_row: u16::try_from(rows.index()).expect("rows fit"),
                ws_col: cols,
            };
            assert_eq!(t.resize_page(&ws), Resized::Ok, "the room is provisioned");
            t.feed(rest.at, rest.len);
        }
    }
    let (bytes, _) = t.amplification();
    assert_eq!(bytes - before, program.len(), "every byte counted once");
}

fn honoured(program: &[u8]) -> Seen {
    honoured_at(program, &[])
}

/// A screen with something on it, a region and side margins, and the cursor
/// away from home: what DECCOLM is meant to clear. DECLRMM stays set: its
/// reset would put the side margins back before DECCOLM could.
const BUSY: &[u8] = b"\x1b[5;10r\x1b[?69h\x1b[3;9s\x1b[1;1Hkept\x1b[3;3Hhere";

const POWER_ON_MODES: &str = "\x1b[?3;2$y\x1b[?40;2$y\x1b[?95;2$y\x1b[?69;2$y";

/// No refusal yet: the reading a fresh terminal has.
fn none_refused() -> ColumnRefusals {
    unheld(b"", usize::MAX).refused
}
const WHOLE_SCREEN: &str = "\x1bP1$r1;24r\x1b\\\x1bP1$r1;80s\x1b\\";

/// **`?40` is reset at power-on and `?3` is ignored under it**: no clear, no
/// margin moved, no home, no request -- xterm's `srm_DECCOLM` does nothing
/// without `c132` -- and the refusal is counted. `DECRQM` answers `2` for
/// all three: each is recognised and acted on.
#[test]
fn deccolm_is_ignored_while_40_is_reset() {
    let before = unheld(BUSY, BUSY.len());
    for program in [[BUSY, b"\x1b[?3h"].concat(), [BUSY, b"\x1b[?3l"].concat()] {
        let after = unheld(&program, program.len());
        assert_ne!(after.refused, before.refused, "{program:?}: counted");
        assert_eq!(
            Seen {
                refused: before.refused,
                ..after
            },
            unheld(BUSY, BUSY.len()),
            "{program:?}: and nothing else"
        );
    }
    let once = unheld(&[BUSY, b"\x1b[?3h"].concat(), usize::MAX).refused;
    let twice = unheld(&[BUSY, b"\x1b[?3h\x1b[?3l"].concat(), usize::MAX).refused;
    assert_ne!(once, twice, "one count a refusal");
    assert_eq!(
        before.modes,
        "\x1b[?3;2$y\x1b[?40;2$y\x1b[?95;2$y\x1b[?69;1$y"
    );
    assert_eq!(before.asked, None);
    assert_eq!(before.refused, none_refused());
    assert_eq!(before.text[0], "kept", "the control: BUSY drew");
    assert_eq!(before.cursor, (2, 6));
}

/// The same refusal, by bytes alone: the screen survives a `?3h` at power-on.
/// Red on the base, which cleared it.
#[test]
fn a_deccolm_at_power_on_leaves_the_screen() {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &SMALL);
    feed(&mut t, b"kept\x1b[3;3H\x1b[?3h");
    assert_eq!(text_of(t.row(Row::ZERO).expect("row 0")), "kept");
    let (r, c) = t.cursor();
    assert_eq!((r.index(), c.index()), (2, 2), "and the cursor stays");
}

/// **Allowed, `?3` is STD 070's `set_column_mode`**: the screen cleared,
/// both margin pairs reset, the cursor at 1;1, `?3` answering set, and 132
/// columns asked for; `?3l` the same with 80. **And DECLRMM reset**, as the
/// VT510 gives it: "DECCOLM resets vertical split screen mode (DECLRMM) to
/// unavailable" (`[[source: vt510-deccolm]]`), where `BUSY` set it.
#[test]
fn deccolm_under_40_clears_resets_both_margin_pairs_homes_and_asks() {
    let set = unheld(&[b"\x1b[?40h", BUSY, b"\x1b[?3h"].concat(), usize::MAX);
    assert_eq!(
        set,
        Seen {
            text: vec![String::new(); 24],
            cursor: (0, 0),
            cols: Cols::of_index(80),
            asked: ASKED_132,
            modes: "\x1b[?3;1$y\x1b[?40;1$y\x1b[?95;2$y\x1b[?69;2$y".into(),
            margins: WHOLE_SCREEN.into(),
            refused: none_refused(),
        }
    );
    let reset = unheld(
        &[b"\x1b[?40h\x1b[?3h", BUSY, b"\x1b[?3l"].concat(),
        usize::MAX,
    );
    assert_eq!(
        reset,
        Seen {
            asked: ASKED_80,
            modes: "\x1b[?3;2$y\x1b[?40;1$y\x1b[?95;2$y\x1b[?69;2$y".into(),
            ..set
        }
    );
}

/// **"even if the terminal was already in the selected state"** (p. 5-71):
/// a second `?3h` clears, resets and homes again, and asks again.
#[test]
fn deccolm_acts_again_in_the_state_it_selects() {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &SMALL);
    feed(&mut t, b"\x1b[?40h\x1b[?3h");
    let first = t.width_requests();
    feed(&mut t, BUSY);
    feed(&mut t, b"\x1b[?3h");
    assert_ne!(t.width_requests(), first, "asked again");
    let s = seen(&mut t);
    assert_eq!(
        (s.text[0].as_str(), s.cursor, s.margins.as_str()),
        ("", (0, 0), WHOLE_SCREEN)
    );
}

/// **Under DECOM the home is still 1;1**, because the margins that would
/// displace it were reset first; the cursor lands on the origin of a whole
/// page.
#[test]
fn deccolm_under_origin_mode_homes_to_one_one() {
    let s = unheld(
        &[b"\x1b[?40h", BUSY, b"\x1b[?6h\x1b[?69h\x1b[3;9s\x1b[?3h"].concat(),
        usize::MAX,
    );
    assert_eq!((s.cursor, s.margins.as_str()), ((0, 0), WHOLE_SCREEN));
}

/// **`?95` withholds the clear and nothing else**: the text stays, and the
/// margins, the home and the request are DECCOLM's as before.
#[test]
fn ncsm_withholds_the_clear_and_nothing_else() {
    let s = unheld(&[b"\x1b[?40;95h", BUSY, b"\x1b[?3h"].concat(), usize::MAX);
    assert_eq!(
        (
            s.text[0].as_str(),
            s.text[2].as_str(),
            s.cursor,
            s.margins.as_str(),
            s.asked,
            s.modes.as_str()
        ),
        (
            "kept",
            "  here",
            (0, 0),
            WHOLE_SCREEN,
            ASKED_132,
            "\x1b[?3;1$y\x1b[?40;1$y\x1b[?95;1$y\x1b[?69;2$y"
        )
    );
    // The control: without `?95` the same program clears.
    let c = unheld(&[b"\x1b[?40h", BUSY, b"\x1b[?3h"].concat(), usize::MAX);
    assert_eq!(c.text[0], "");
}

/// **DECSCPP asks whatever `?40` says, and does nothing else here**: no
/// clear, no margin moved, no cursor moved, and `?3` not moved -- STD 070's
/// `set_columns_per_page` changes `columns_per_page`, not `column_mode`.
#[test]
fn decscpp_asks_without_40_and_leaves_the_screen() {
    let busy = unheld(BUSY, usize::MAX);
    let s = unheld(&[BUSY, b"\x1b[132$|"].concat(), usize::MAX);
    assert_eq!(
        s,
        Seen {
            asked: ASKED_132,
            ..busy
        }
    );
}

/// **The parameter is STD 070's**: `Pn <= 80` is 80 -- omitted and 0 among
/// them -- and anything past it 132, saturated values too. xterm ignores
/// all but 0, 80 and 132. A private `CSI ? Pn $ |` is no control and asks
/// nothing.
#[test]
fn decscpp_reads_its_parameter_as_std_070_does() {
    for (seq, want) in [
        (&b"\x1b[$|"[..], ASKED_80),
        (b"\x1b[0$|", ASKED_80),
        (b"\x1b[1$|", ASKED_80),
        (b"\x1b[80$|", ASKED_80),
        (b"\x1b[81$|", ASKED_132),
        (b"\x1b[100$|", ASKED_132),
        (b"\x1b[132$|", ASKED_132),
        (b"\x1b[133$|", ASKED_132),
        (b"\x1b[99999$|", ASKED_132),
        (b"\x1b[?132$|", None),
    ] {
        assert_eq!(unheld(seq, usize::MAX).asked, want, "{seq:?}");
    }
}

/// **In an 80-column window `RIS` asks for 80 after a 132 and for nothing
/// otherwise**, and resets `?40` and `?95`; `DECSTR` keeps both and asks
/// for nothing. (A window of another width: the test after the held ones.)
#[test]
fn ris_in_an_80_column_window_asks_for_80_and_resets_the_modes() {
    for (name, program, want) in [
        ("?3h", &b"\x1b[?40;95h\x1b[?3h\x1bc"[..], ASKED_80),
        ("DECSCPP 132", b"\x1b[132$|\x1bc", ASKED_80),
        ("never asked", b"\x1b[?40;95h\x1bc", None),
    ] {
        let s = unheld(program, usize::MAX);
        assert_eq!(
            (s.asked, s.modes.as_str()),
            (want, POWER_ON_MODES),
            "{name}"
        );
    }
    // An 80 already asked is not asked again.
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    feed(&mut t, b"\x1b[80$|");
    let before = t.width_requests();
    feed(&mut t, b"\x1bc");
    assert_eq!(t.width_requests(), before, "80 was the last asked");
    let s = unheld(b"\x1b[?40;95h\x1b[!p", usize::MAX);
    assert_eq!(
        (s.asked, s.modes.as_str()),
        (None, "\x1b[?3;2$y\x1b[?40;1$y\x1b[?95;1$y\x1b[?69;2$y"),
        "DECSTR"
    );
}

/// **XTSAVE keeps `?40` and `?95`, and XTRESTORE brings them back**, "Ps
/// values are the same as for DECSET".
#[test]
fn xtsave_and_xtrestore_carry_40_and_95() {
    let s = unheld(
        b"\x1b[?40;95h\x1b[?40;95s\x1b[?40;95l\x1b[?40;95r",
        usize::MAX,
    );
    assert_eq!(s.modes, "\x1b[?3;2$y\x1b[?40;1$y\x1b[?95;1$y\x1b[?69;2$y");
}

/// **XTRESTORE of `?3` goes through DECSET's path, so it acts only under
/// `?40`**: saved set under `?40`, restored after `?40` was reset, it is
/// refused -- the text drawn after the save survives and the refusal is
/// counted -- and restored with `?40` set it clears and asks.
#[test]
fn xtrestore_of_3_acts_only_under_40() {
    let prep: &[u8] = b"\x1b[?40h\x1b[?3h\x1b[?3s\x1b[?3l\x1b[?40ltext";
    let before = unheld(prep, usize::MAX);
    let refused = unheld(&[prep, b"\x1b[?3r"].concat(), usize::MAX);
    assert_eq!(refused.text[0], "text", "the restore cleared nothing");
    assert_ne!(refused.refused, before.refused, "and was counted");
    assert_eq!(refused.asked, ASKED_80, "the ?3l before it");
    let allowed = unheld(&[prep, b"\x1b[?40h\x1b[?3r"].concat(), usize::MAX);
    assert_eq!((allowed.text[0].as_str(), allowed.asked), ("", ASKED_132));
}

/// **The modes and the request land the same at every read size and every
/// split into two reads**, to a terminal that never resizes.
#[test]
fn the_column_modes_land_the_same_at_every_read_split() {
    let program: &[u8] = b"\x1b[?95h\x1b[?3h\x1b[?40h\x1b[?95l\x1b[?40;95h\x1b[?95l\
        \x1b[5;10rtext\x1b[?3h\x1b[100$|after";
    let want = unheld(program, usize::MAX);
    assert_eq!(
        (want.asked, want.text[0].as_str()),
        (ASKED_132, "after"),
        "the whole read"
    );
    assert_ne!(want.refused, none_refused(), "the first ?3 was refused");
    for size in 1..=program.len() {
        assert_eq!(unheld(program, size), want, "read size {size}");
    }
    for cut in 1..program.len() {
        let mut s = Owned::new(&ROOM);
        let mut t = Term::new(s.storage(), &SMALL);
        feed(&mut t, &program[..cut]);
        feed(&mut t, &program[cut..]);
        assert_eq!(seen(&mut t), want, "split at {cut}");
    }
}

/// What a vttest-like program draws once it has asked for 132 columns: a
/// mark at the last column and one past 80.
const WIDE: &[u8] = b"\x1b[?40h\x1b[?3h\x1b[1;132HX\x1b[2;100HY\x1b[3;1Hz";

/// **The bytes after the request land at the width asked for**, at every
/// read size and every split: the read stops at `?3h`, the embedder resizes,
/// and the rest is fed. Unheld, the same read puts the marks at the old
/// width's edge -- the control that says the hold is what moves them.
#[test]
fn a_held_read_lands_the_rest_at_the_width_asked_for() {
    let want = honoured(WIDE);
    let mut row0 = " ".repeat(131);
    row0.push('X');
    let mut row1 = " ".repeat(99);
    row1.push('Y');
    assert_eq!(
        (
            want.cols,
            &want.text[0],
            &want.text[1],
            want.text[2].as_str()
        ),
        (Cols::of_index(132), &row0, &row1, "z"),
        "the whole read"
    );
    let unheld = unheld(WIDE, usize::MAX);
    assert_eq!(
        (unheld.cols, unheld.text[0].len(), unheld.text[1].len()),
        (Cols::of_index(80), 80, 80),
        "the control: a terminal that does not hold draws at 80"
    );
    for size in 1..=WIDE.len() {
        let cuts: Vec<usize> = (size..WIDE.len()).step_by(size).collect();
        assert_eq!(honoured_at(WIDE, &cuts), want, "read size {size}");
    }
    for cut in 1..WIDE.len() {
        assert_eq!(honoured_at(WIDE, &[cut]), want, "split at {cut}");
    }
}

/// **DECSCPP holds too**, and two requests in one read stop it twice. The
/// text before it stays, at the new width, and the `X` drawn at column 100
/// is gone: "When changing from a 132 column to a 80 column page, the
/// information in columns 1-80 is preserved. The information in columns
/// 81-132 is cleared" (`[[source: dec-std-070]]` p. 5-73).
#[test]
fn decscpp_holds_and_two_requests_stop_a_read_twice() {
    let program: &[u8] = b"left\x1b[132$|\x1b[1;100HX\x1b[80$|\x1b[3;1Hy";
    let s = honoured(program);
    assert_eq!(
        (
            s.cols,
            s.text[0].as_str(),
            s.text[1].as_str(),
            s.text[2].as_str()
        ),
        (Cols::of_index(80), "left", "", "y"),
        "back to 80, and the X cleared, not re-cut onto the next row"
    );
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    t.set_width_hold(WidthHold::On);
    feed(&mut t, program);
    let first = t.take_held().expect("the first request held the read");
    assert_eq!(first.at.index(), 11, "after `left` and `CSI 132 $ |`");
    assert_eq!(first.len.index(), program.len() - 11);
    assert_eq!(t.take_held(), None, "taken once");
}

/// **"DECSCPP does not cause the Active position to move. However if the
/// active position is beyond the width of the new page when the command
/// is executed, the active position will be moved to the right edge of the
/// page"** (`[[source: dec-std-070]]` p. 5-73): kept at column 40 either
/// way, and moved from column 100 to 80 on its own row, not carried to the
/// next by a re-cut.
#[test]
fn decscpp_moves_the_cursor_only_past_the_new_edge() {
    for (program, want) in [
        (&b"\x1b[132$|\x1b[5;40H\x1b[80$|"[..], (4, 39)),
        (b"\x1b[132$|\x1b[5;100H\x1b[80$|", (4, 79)),
        (b"\x1b[5;40H\x1b[132$|", (4, 39)),
    ] {
        assert_eq!(honoured(program).cursor, want, "{program:?}");
    }
}

/// **"DECSCPP does not cause the Right Margin to move. However if the right
/// margin is beyond the width of the new page when the command is executed,
/// the right margin will be moved to the right edge of the page"**
/// (`[[source: dec-std-070]]` p. 5-73), and its "State Affected" names the
/// right margin and not the others (p. 5-74), so DECSTBM's pair and the
/// left margin stay; "DECSCPP does not change the setting of DECLRMM"
/// (note 3). The right margin at 120 goes to 80; one at 60 stays on a
/// narrowing and on a widening; one at 80 stays at 80 on a widening, as the
/// algorithm moves it on the narrowing alone.
#[test]
fn decscpp_keeps_the_margins_but_a_right_one_past_the_edge() {
    let margins = |r: &str, s: &str| format!("\x1bP1$r{r}r\x1b\\\x1bP1$r{s}s\x1b\\");
    for (program, want) in [
        (
            &b"\x1b[132$|\x1b[3;20r\x1b[?69h\x1b[5;120s\x1b[80$|"[..],
            margins("3;20", "5;80"),
        ),
        (
            b"\x1b[132$|\x1b[3;20r\x1b[?69h\x1b[5;60s\x1b[80$|",
            margins("3;20", "5;60"),
        ),
        (
            b"\x1b[3;20r\x1b[?69h\x1b[5;60s\x1b[132$|",
            margins("3;20", "5;60"),
        ),
        (b"\x1b[?69h\x1b[132$|", margins("1;24", "1;80")),
    ] {
        let s = honoured(program);
        assert_eq!(s.margins, want, "{program:?}");
        assert!(
            s.modes.contains("\x1b[?69;1$y"),
            "{program:?}: {:?}",
            s.modes
        );
    }
}

/// **A left margin the new right one no longer clears goes, with it, to the
/// page's edges**: DECSLRM 100;120 on a 132-column page, then DECSCPP 80.
/// The text moves the right margin to 80 and leaves the left at 100, which
/// no DECSLRM can set; baud keeps no pair a margin cannot hold.
#[test]
fn decscpp_past_both_side_margins_resets_the_pair() {
    let s = honoured(b"\x1b[132$|\x1b[?69h\x1b[100;120s\x1b[80$|");
    assert_eq!(s.margins, WHOLE_SCREEN);
}

/// **The margins kept fence the writes as they did**: inside DECSTBM's
/// area a line wraps at the right margin kept, 60, and outside it a line
/// runs to the line's end (`[[source: dec-std-070]]` p. 5-139), each
/// twelve characters from column 55. A wrap pending at the right margin
/// stays pending through the narrowing, and the next character goes to the
/// left margin kept, column 5, on the next line.
#[test]
fn decscpp_kept_margins_fence_the_writes_as_before() {
    let s = honoured(
        b"\x1b[132$|\x1b[?69h\x1b[1;10r\x1b[5;60s\x1b[80$|\
          \x1b[5;55Habcdefghijkl\x1b[15;55Habcdefghijkl",
    );
    let pad = |n: usize, t: &str| format!("{}{t}", " ".repeat(n));
    assert_eq!(
        (s.text[4].as_str(), s.text[5].as_str(), s.text[14].as_str()),
        (
            pad(54, "abcdef").as_str(),
            pad(4, "ghijkl").as_str(),
            pad(54, "abcdefghijkl").as_str()
        )
    );
    let s = honoured(b"\x1b[132$|\x1b[?69h\x1b[5;60s\x1b[2;50Habcdefghijk\x1b[80$|Z");
    assert_eq!(
        (s.text[1].as_str(), s.text[2].as_str()),
        (pad(49, "abcdefghijk").as_str(), pad(4, "Z").as_str()),
        "the wrap pending at the margin kept"
    );
}

/// **"The information in columns 81-132 is cleared"** and "the information
/// in columns 1-80 is preserved" (`[[source: dec-std-070]]` p. 5-73): text
/// at column 1, an `X` at column 100 and `AB` on columns 79 and 80, then
/// 80 and 132 again. The `X` is not there to come back; `AB` is where it
/// was.
#[test]
fn decscpp_clears_columns_81_to_132_and_keeps_1_to_80() {
    let s = honoured(b"\x1b[132$|kept\x1b[1;100HX\x1b[2;79HAB\x1b[80$|\x1b[132$|");
    assert_eq!(
        (
            s.cols,
            s.text[0].as_str(),
            s.text[1].as_str(),
            s.text[2].as_str()
        ),
        (
            Cols::of_index(132),
            "kept",
            format!("{}AB", " ".repeat(78)).as_str(),
            ""
        )
    );
}

/// **"When changing from an 80 column page to a 132 column page, the
/// information in columns 1-80 is preserved"** (`[[source: dec-std-070]]`
/// p. 5-73): a line wrapped at 80 stays two rows at 132. The control: the
/// same widening asked by `?3 h` under `?95` is answered as a window's, and
/// re-cuts the two rows into one.
#[test]
fn decscpp_widening_keeps_1_to_80_where_they_are() {
    let line = [[b'a'; 80].as_slice(), &[b'b'; 10]].concat();
    let s = honoured(&[line.as_slice(), b"\x1b[132$|"].concat());
    let a = "a".repeat(80);
    assert_eq!(
        (s.cols, s.text[0].as_str(), s.text[1].as_str()),
        (Cols::of_index(132), a.as_str(), "bbbbbbbbbb")
    );
    let c = honoured(&[line.as_slice(), b"\x1b[?40;95h\x1b[?3h"].concat());
    assert_eq!(
        (c.cols, c.text[0].as_str(), c.text[1].as_str()),
        (Cols::of_index(132), format!("{a}bbbbbbbbbb").as_str(), ""),
        "the control: DECCOLM's answer re-cuts"
    );
}

/// **The page is the screen; the history is kept as written**: a line with
/// an `X` at column 100 scrolled behind the window keeps it through 80 and
/// back to 132, where the page's rows lose theirs. The page's alternate
/// screen loses its `X` too.
#[test]
fn decscpp_clears_the_page_and_keeps_the_history() {
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    let lines = b"\r\n".repeat(24);
    let program = [
        b"\x1b[132$|\x1b[1;100HX".as_slice(),
        lines.as_slice(),
        b"\x1b[1;100HX\x1b[80$|\x1b[132$|",
    ]
    .concat();
    honour_into(&mut t, &program, &[]);
    let kept = t.scrollback().line(Back::of(0)).map(|(c, _)| text_of(c));
    assert_eq!(kept, Some(format!("{}X", " ".repeat(99))), "the history");
    assert_eq!(
        text_of(t.row(Row::ZERO).expect("a row on the grid")),
        "",
        "the page"
    );
    let alt = honoured(b"\x1b[132$|\x1b[?1049h\x1b[1;100HX\x1b[80$|\x1b[132$|");
    assert_eq!(alt.text[0], "", "the alternate screen");
}

/// **DECSCPP's answer re-cuts no line and tells none**
/// (`Term::resize_page_with`), where `?3`'s is a window's resize.
#[test]
fn decscpp_answer_tells_no_line() {
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    t.set_width_hold(WidthHold::On);
    let program = [[b'a'; 100].as_slice(), b"\x1b[132$|"].concat();
    feed(&mut t, &program);
    let rest = t.take_held().expect("the request held the read");
    let mut told = Vec::new();
    let ws = WinSize {
        ws_row: SMALL.ws_row,
        ws_col: 132,
    };
    let answer = t.resize_page_with(&ws, &mut |r| told.push(r));
    assert_eq!(answer, (Resized::Ok, Following::Followed));
    assert!(told.is_empty(), "{told:?}");
    t.feed(rest.at, rest.len);
    assert_eq!(t.geometry().0, Cols::of_index(132));
}

/// **A request that is the read's last byte holds an empty rest**, and a
/// rest no one took is gone when the next read starts.
#[test]
fn a_request_at_the_end_of_a_read_holds_an_empty_rest() {
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    t.set_width_hold(WidthHold::On);
    feed(&mut t, b"\x1b[?40h\x1b[?3h");
    let rest = t.take_held().expect("held");
    assert_eq!((rest.at.index(), rest.len.index()), (11, 0));
    feed(&mut t, b"\x1b[132$|more");
    feed(&mut t, b"plain");
    assert_eq!(t.take_held(), None, "the untaken rest went with its read");
}

/// **A held read's tail ends at the request**, so a hide at the start of
/// the rest looks back at the request and not at the read's own last bytes
/// (`placed.rs`): here those spell a `CUP`, and a tail cut at the read's
/// end would place the cursor. The control: a `CUP` straight before a hide
/// does place it, across a read boundary too.
#[test]
fn a_held_reads_tail_ends_at_the_request() {
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    t.set_width_hold(WidthHold::On);
    feed(&mut t, b"\x1b[1;1H\x1b[132$|\x1b[?25l\x1b[5;5H");
    let rest = t.take_held().expect("held");
    t.feed(rest.at, rest.len);
    assert_eq!(t.placed_cursor(), None);
    feed(&mut t, b"\x1b[?25h\x1b[3;3H");
    feed(&mut t, b"\x1b[?25l");
    assert_eq!(
        t.placed_cursor(),
        Some((Row::from_index(2), Col::at_index(2))),
        "the control"
    );
}

/// **`RIS` asks without holding**: an `ESC` final is not stopped at, so the
/// read goes on at the old width, a known gap.
#[test]
fn ris_asks_without_holding() {
    let mut o = Owned::new(&ROOM);
    let mut t = Term::new(o.storage(), &SMALL);
    t.set_width_hold(WidthHold::On);
    feed(&mut t, b"\x1b[132$|");
    assert!(t.take_held().is_some());
    feed(&mut t, b"\x1bcafter");
    assert_eq!((t.requested_width(), t.take_held()), (ASKED_80, None));
    assert_eq!(text_of(t.row(Row::ZERO).expect("row 0")), "after");
}

/// What a held read leaves when the parser's own state moves in the
/// request's sequence: the protocol, the cursor, the width, and the width
/// each held rest was fed at. **Not `seen`**, whose `DECRQM` and `DECRQSS` strings a
/// VT52 parser reads as VT52 commands. The terminal starts at 132 columns;
/// honoured, each held rest is fed at the width asked for, and a rest that
/// holds again more than eight times fails rather than looping: the parser
/// resumed in `Node::Held` itself.
fn resumed(hold: WidthHold, start: WinSize, program: &[u8], cuts: &[usize]) -> Resumed {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &start);
    t.set_width_hold(hold);
    let mut holds = Vec::new();
    let mut from = 0;
    for &to in cuts.iter().chain(core::iter::once(&program.len())) {
        let chunk = &program[from..to];
        from = to;
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        while let Some(rest) = t.take_held() {
            assert!(
                holds.len() < 8,
                "a held rest held again: {program:?} {cuts:?}"
            );
            let ws = WinSize {
                ws_row: start.ws_row,
                ws_col: t.requested_width().expect("a held read asked").ws_col(),
            };
            assert_eq!(t.resize_page(&ws), Resized::Ok, "the room is provisioned");
            holds.push(t.geometry().0);
            t.feed(rest.at, rest.len);
        }
    }
    let (r, c) = t.cursor();
    Resumed {
        mode: t.terminal_mode(),
        cursor: (r.index(), c.index()),
        cols: t.geometry().0,
        holds,
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Resumed {
    mode: TerminalMode,
    cursor: (usize, usize),
    cols: Cols,
    holds: Vec<Cols>,
}

/// **DECANM and a held `?3` in one sequence: the rest is VT52's, at the
/// width asked for**, in either order, at every read size and every split.
/// `ESC Y % ~` addresses row 5, column 94 (VT52's coordinates are offset
/// by 32): at the 80 columns `?3l` asked for it lands on column 79, and
/// unheld, at the 132 the terminal has, on 94 -- the control, which says
/// what the hold moves and that both orders leave ANSI. `?3;2;3l` parks
/// twice in one sequence, and the second must not take `Held` as the node
/// the rest goes on from.
#[test]
fn decanm_beside_a_held_deccolm_resumes_in_vt52() {
    for program in [
        &b"\x1b[?40h\x1b[?2;3l\x1bY%~"[..],
        b"\x1b[?40h\x1b[?3;2l\x1bY%~",
        b"\x1b[?40h\x1b[?3;2;3l\x1bY%~",
    ] {
        let want = Resumed {
            mode: TerminalMode::Vt52,
            cursor: (5, 79),
            cols: Cols::of_index(80),
            holds: vec![Cols::of_index(80)],
        };
        assert_eq!(
            resumed(WidthHold::Off, ROOM, program, &[]),
            Resumed {
                cursor: (5, 94),
                cols: Cols::of_index(132),
                holds: Vec::new(),
                ..want.clone()
            },
            "the control, unheld: {program:?}"
        );
        assert_eq!(
            resumed(WidthHold::On, ROOM, program, &[]),
            want,
            "{program:?} whole"
        );
        for size in 1..=program.len() {
            let cuts: Vec<usize> = (size..program.len()).step_by(size).collect();
            assert_eq!(
                resumed(WidthHold::On, ROOM, program, &cuts),
                want,
                "{program:?} read size {size}"
            );
        }
        for cut in 1..program.len() {
            assert_eq!(
                resumed(WidthHold::On, ROOM, program, &[cut]),
                want,
                "{program:?} split at {cut}"
            );
        }
    }
}

/// **XTRESTORE of `?2;3` holds at `?3` and goes on in ANSI.** A `CSI` is
/// parsed only in ANSI, so XTSAVE can only ever save `?2` set, and
/// XTRESTORE replays only a mode whose value differs from the one saved:
/// `?2` is never replayed, and the restore cannot leave ANSI. The rest lands
/// at the width the restored `?3` asked for, at every split; unheld, at the
/// 80 the terminal kept.
#[test]
fn xtrestore_of_2_and_3_holds_and_resumes_in_ansi() {
    let program: &[u8] = b"\x1b[?40h\x1b[?3h\x1b[?2;3s\x1b[?3l\x1b[?2;3r\x1b[1;100HX";
    let want = Resumed {
        mode: TerminalMode::Ansi,
        cursor: (0, 100),
        cols: Cols::of_index(132),
        holds: [132, 80, 132].map(Cols::of_index).to_vec(),
    };
    assert_eq!(
        resumed(WidthHold::Off, SMALL, program, &[]),
        Resumed {
            cursor: (0, 79),
            cols: Cols::of_index(80),
            holds: Vec::new(),
            ..want.clone()
        },
        "the control, unheld"
    );
    assert_eq!(resumed(WidthHold::On, SMALL, program, &[]), want, "whole");
    for size in 1..=program.len() {
        let cuts: Vec<usize> = (size..program.len()).step_by(size).collect();
        assert_eq!(
            resumed(WidthHold::On, SMALL, program, &cuts),
            want,
            "read size {size}"
        );
    }
    for cut in 1..program.len() {
        assert_eq!(
            resumed(WidthHold::On, SMALL, program, &[cut]),
            want,
            "split at {cut}"
        );
    }
}

/// `DECRQSS` for `$|` in a terminal `start` wide, after `program`, unheld.
fn columns_per_page(start: &WinSize, program: &[u8]) -> String {
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), start);
    feed(&mut t, program);
    t.clear_reply();
    feed(&mut t, b"\x1bP$q$|\x1b\\");
    String::from_utf8_lossy(t.reply()).into_owned()
}

/// **`DECRQSS` answers DECSCPP** -- STD 070's DECRQSS lists "Set Columns
/// Per Page DECSCPP $ |" among what it reports -- **with the page the grid
/// is**, by DECSCPP's own rule: up to 80 columns is 80, wider is 132. So a
/// request the embedder did not honour is not reported as the page: asked
/// for 132 and kept at 80, the answer is 80. The control: `DECRQSS r`
/// answers beside it.
#[test]
fn decrqss_answers_decscpp_with_the_grids_page() {
    let at = |cols| WinSize {
        ws_row: 24,
        ws_col: cols,
    };
    for (start, program, want) in [
        (SMALL, &b""[..], "\x1bP1$r80$|\x1b\\"),
        (ROOM, b"", "\x1bP1$r132$|\x1b\\"),
        (at(100), b"", "\x1bP1$r132$|\x1b\\"),
        (at(40), b"", "\x1bP1$r80$|\x1b\\"),
        (SMALL, b"\x1b[132$|", "\x1bP1$r80$|\x1b\\"),
        (SMALL, b"\x1b[?40h\x1b[?3h", "\x1bP1$r80$|\x1b\\"),
    ] {
        assert_eq!(
            columns_per_page(&start, program),
            want,
            "{} columns, {program:?}",
            start.ws_col
        );
    }
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &SMALL);
    feed(&mut t, b"\x1bP$q$|\x1b\\\x1bP$qr\x1b\\");
    assert_eq!(
        String::from_utf8_lossy(t.reply()),
        "\x1bP1$r80$|\x1b\\\x1bP1$r1;24r\x1b\\"
    );
}

/// **`RIS` asks for the width the window had before the first request**:
/// STD 070's `RIS` is its Device Initialization, whose `host_state_init`
/// reads `column mode = EIGHTY; /* NVM */` -- the width the user stored --
/// and whose values "apply only to those implementations in which the state
/// cannot be set and stored locally by the terminal user". A window 100
/// columns wide that a program took to 132 goes back to 100, not to 80; one
/// no program asked of is asked nothing; and the width is remembered from
/// the first request since power-on or the last `RIS`.
#[test]
fn ris_asks_for_the_width_before_the_first_request() {
    let hundred = WinSize {
        ws_row: 24,
        ws_col: 100,
    };
    let asked = |program: &[u8]| {
        let mut s = Owned::new(&ROOM);
        let mut t = Term::new(s.storage(), &hundred);
        t.set_width_hold(WidthHold::On);
        let mut from = 0;
        let mut requests = Vec::new();
        for (i, &b) in program.iter().enumerate() {
            if b != b'\n' {
                continue;
            }
            let chunk = &program[from..i];
            from = i + 1;
            let before = t.width_requests();
            t.write_at(BytePos::ZERO, chunk);
            t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
            // As the macOS app does: a held read is resized at the
            // request, and any other request after the read.
            while let Some(rest) = t.take_held() {
                let ws = WinSize {
                    ws_row: 24,
                    ws_col: t.requested_width().expect("asked").ws_col(),
                };
                assert_eq!(t.resize(&ws), Resized::Ok);
                t.feed(rest.at, rest.len);
            }
            if t.width_requests() != before {
                let ws = WinSize {
                    ws_row: 24,
                    ws_col: t.requested_width().expect("asked").ws_col(),
                };
                assert_eq!(t.resize(&ws), Resized::Ok);
            }
            requests.push(
                (t.width_requests() != before)
                    .then(|| t.requested_width().expect("asked").ws_col()),
            );
        }
        requests
    };
    assert_eq!(
        asked(b"\x1b[132$|\n\x1bc\n"),
        [Some(132), Some(100)],
        "back to the window's 100"
    );
    assert_eq!(
        asked(b"\x1b[?40h\x1b[?3h\x1b[?3l\n\x1bc\n"),
        [Some(80), Some(100)],
        "from 80 too"
    );
    assert_eq!(asked(b"\x1bc\n"), [None], "no request, nothing asked");
    assert_eq!(
        asked(b"\x1b[100$|\n\x1bc\n\x1bc\n"),
        [Some(132), Some(100), None],
        "asked once a request"
    );
    assert_eq!(
        asked(b"\x1b[80$|\n\x1bc\n\x1b[132$|\n\x1bc\n"),
        [Some(80), Some(100), Some(132), Some(100)],
        "remembered again after a RIS"
    );
    // **And noted afresh after a `RIS`**: a width the user gives the window
    // between two programs is the one the second `RIS` asks back for.
    let mut s = Owned::new(&ROOM);
    let mut t = Term::new(s.storage(), &hundred);
    for (program, then) in [(&b"\x1b[132$|"[..], 132), (b"\x1bc", 100), (b"", 120)] {
        feed(&mut t, program);
        let ws = WinSize {
            ws_row: 24,
            ws_col: then,
        };
        assert_eq!(t.resize(&ws), Resized::Ok);
    }
    feed(&mut t, b"\x1b[80$|\x1bc");
    assert_eq!(
        t.requested_width().map(WidthRequest::ws_col),
        Some(120),
        "the user's 120, not the first program's 100"
    );
}
