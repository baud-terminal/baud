//! The xterm private modes that name a resource (`[[source: ctlseqs]]`):
//! the switches a program sets -- `?1010`, `?1011`, `?1040`, `?1041`, `?1045`,
//! `?1046`, `?2001`, `?2005`, `?2006` -- the read-only `?13` and `?14`, the
//! permanent `?1014` and `?1044`, and `?2002` and `?2003`, not recognised.
//! `DECRQM` answers each truthfully.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{CursorShape, CursorStyle};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 5,
};

fn decrqm(t: &mut Term<'_>, mode: u16) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, format!("\x1b[?{mode}$p").as_bytes());
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

fn answer(mode: u16, value: u8) -> Vec<u8> {
    format!("\x1b[?{mode};{value}$y").into_bytes()
}

/// **What each mode answers at power-on**: the switches their power-on
/// values (`?1011`, `?1040` and `?1046` set, the rest reset), `?13` the configured
/// blink, `?14` permanently reset (baud ORs `?12` with the style's blink
/// and never XORs), `?1014` and `?1044` permanently set, `?2002` and
/// `?2003` not recognised.
#[test]
fn each_mode_answers_what_baud_does_at_power_on() {
    support::with_term(&WS, |t| {
        for (mode, value) in [
            (13, 1),
            (14, 4),
            (1010, 2),
            (1011, 1),
            (1014, 3),
            (1040, 1),
            (1041, 2),
            (1044, 3),
            (1045, 2),
            (1046, 1),
            (2001, 2),
            (2002, 0),
            (2003, 0),
            (2005, 2),
            (2006, 2),
        ] {
            assert_eq!(decrqm(t, mode), answer(mode, value), "?{mode}");
        }
    });
}

/// `?13` is the blink the embedder configured, `2` for a steady cursor,
/// and no program can change it: DECSET of `?13`, `?14`, `?1014` or
/// `?1044` is counted and changes no answer.
#[test]
fn the_read_only_and_permanent_modes_do_not_move() {
    support::with_term(&WS, |t| {
        t.set_initial_cursor_style(CursorStyle::of(
            CursorShape::Bar,
            baud_domain::CursorBlink::Steady,
        ));
        assert_eq!(decrqm(t, 13), answer(13, 2));
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[?13h\x1b[?14h\x1b[?1014l\x1b[?1044l");
        assert_eq!(t.unsupported_param, counted + 4);
        for (mode, value) in [(13, 2), (14, 4), (1014, 3), (1044, 3)] {
            assert_eq!(decrqm(t, mode), answer(mode, value), "?{mode}");
        }
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[?2002h\x1b[?2003h");
        assert_eq!(t.unsupported_param, counted + 2, "not recognised");
    });
}

/// **Each switch sets and resets, and the embedder reads it**: DECRQM,
/// `Term::resource`, XTSAVE and XTRESTORE agree; `RIS` puts the power-on
/// values back and `DECSTR` keeps what a program set.
#[test]
fn each_switch_sets_resets_saves_and_restores() {
    use Resource::*;
    let all = [
        (1010, ScrollOnOutput, false),
        (1011, ScrollOnKey, true),
        (1040, KeepSelection, true),
        (1041, SelectToClipboard, false),
        (1045, ReverseWrapExtended, false),
        (1046, AltScreenAllowed, true),
        (2001, ClickMovesPoint, false),
        (2005, PasteQuoted, false),
        (2006, PasteLiteralNewline, false),
    ];
    support::with_term(&WS, |t| {
        for (mode, r, on) in all {
            assert_eq!(t.resource(r), on, "?{mode} at power-on");
            let flip = if on { 'l' } else { 'h' };
            support::feed(t, format!("\x1b[?{mode}s\x1b[?{mode}{flip}").as_bytes());
            assert_eq!(t.resource(r), !on, "?{mode} flipped");
            assert_eq!(decrqm(t, mode), answer(mode, if on { 2 } else { 1 }));
            support::feed(t, format!("\x1b[?{mode}r").as_bytes());
            assert_eq!(t.resource(r), on, "?{mode} restored");
            support::feed(t, format!("\x1b[?{mode}{flip}\x1b[!p").as_bytes());
            assert_eq!(t.resource(r), !on, "?{mode} kept by DECSTR");
            support::feed(t, b"\x1bc");
            assert_eq!(t.resource(r), on, "?{mode} back at RIS");
        }
    });
}

/// Forty lines on a five-row screen, the window three lines back.
fn scrolled_back(t: &mut Term<'_>) {
    let lines: Vec<u8> = (0..40)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    support::feed(t, &lines);
    t.set_window_back(Rows::of_count(3));
    assert_eq!(t.window_back().index(), 3);
}

/// **`?1010`: output sends the window live**, once a read; reset, the
/// default, the window keeps its line while output arrives. A program on
/// the alternate screen leaves the primary's window where it was.
#[test]
fn scroll_on_output_sends_the_window_live_when_set() {
    support::with_term(&WS, |t| {
        scrolled_back(t);
        support::feed(t, b"more\r\n");
        assert_ne!(t.window_back().index(), 0, "reset keeps the window");
        support::feed(t, b"\x1b[?1010h");
        support::feed(t, b"x");
        assert_eq!(t.window_back().index(), 0, "set sends it live");
        support::feed(t, b"\x1b[?1049h");
        t.set_window_back(Rows::of_count(3));
        support::feed(t, b"on the alternate screen");
        assert_eq!(t.primary_window_back().index(), 3, "the primary's kept");
    });
}

/// **`?1045`, XTREVWRAP2, wraps backspace past the left margin**: to the line above's
/// end, and from the top margin to the bottom margin's line, while `?7` is
/// set; with `?7` reset it does nothing.
#[test]
fn extended_reverse_wrap_goes_up_and_round_from_the_top() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1045h\x1b[3;1H\x08");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(19)));
        support::feed(t, b"\x1b[1;1H\x08");
        assert_eq!(t.cursor(), (Row::from_index(4), Col::at_index(19)));
        support::feed(t, b"\x1b[2;4r\x1b[2;1H\x08");
        assert_eq!(
            t.cursor(),
            (Row::from_index(3), Col::at_index(19)),
            "the region's top goes to its bottom"
        );
        support::feed(t, b"\x1b[r\x1b[?7l\x1b[3;1H\x08");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::ZERO), "?7 reset");
    });
}

/// **`CUB` under `?1045` wraps as backspace does, its count included**,
/// as xterm's `CursorBack` serves both (`[[source: xterm-cursor-c]]`;
/// `[[source: xterm-reset]]`, `CASE_CUB`): each step past the left
/// margin goes to the end of the line above, and from the top margin to
/// the bottom margin's line.
#[test]
fn cub_under_extended_reverse_wrap_wraps_a_count_of_columns() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1045h\x1b[3;1H\x1b[D");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(19)));
        support::feed(t, b"\x1b[3;3H\x1b[5D");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(17)));
        support::feed(t, b"\x1b[3;1H\x1b[45D");
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(15)),
            "across two lines and round from the top"
        );
        support::feed(t, b"\x1b[3;1H\x1b[65535D");
        assert_eq!(t.cursor(), (Row::from_index(0), Col::at_index(5)), "laps");
        support::feed(t, b"\x1b[2;4r\x1b[2;1H\x1b[D");
        assert_eq!(
            t.cursor(),
            (Row::from_index(3), Col::at_index(19)),
            "the region's top goes to its bottom"
        );
        support::feed(t, b"\x1b[1;3H\x1b[5D");
        assert_eq!(
            t.cursor(),
            (Row::from_index(0), Col::ZERO),
            "above the top margin it stops on the screen"
        );
        support::feed(t, b"\x1b[r\x1b[?7l\x1b[3;1H\x1b[D");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::ZERO), "?7 reset");
    });
}

/// From a pending wrap the first step is the wrap's own column, as xterm's
/// `CursorBack` takes one from the count under `?1045` with `?7`, `(rev ||
/// rev2) && screen->do_wrap` (`[[source: xterm-cursor-c]]` 153), where
/// `CUB` without it counts from the last column; under `DECLRMM` the walk
/// turns at the side margins, and from left of the left margin at the
/// screen's first column.
#[test]
fn cub_under_extended_reverse_wrap_from_a_pending_wrap_and_in_margins() {
    support::with_term(&WS, |t| {
        for (n, at) in [(1, 19), (2, 18), (3, 17)] {
            let p = format!("\x1b[?1045h\x1b[1;1Habcdefghijklmnopqrst\x1b[{n}D");
            support::feed(t, p.as_bytes());
            assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(at)), "CUB {n}");
        }
        support::feed(t, b"\x1b[?69h\x1b[5;10s\x1b[3;5H\x1b[D");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(9)));
        support::feed(t, b"\x1b[3;5H\x1b[3D");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(7)));
        support::feed(t, b"\x1b[3;2H\x1b[3D");
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(8)),
            "left of the margin the walk turns at column 1"
        );
    });
}

/// **`?1046` reset holds the screen**: it leaves the alternate screen, and
/// `?1049` and `?47` then switch nothing until it is set again.
#[test]
fn alt_screen_switching_can_be_turned_off() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1049h");
        assert_eq!(t.screen(), Screen::Alternate);
        support::feed(t, b"\x1b[?1046l");
        assert_eq!(t.screen(), Screen::Normal, "reset leaves it");
        support::feed(t, b"\x1b[?1049h\x1b[?47h");
        assert_eq!(t.screen(), Screen::Normal, "no switch while reset");
        support::feed(t, b"\x1b[?1046h\x1b[?1049h");
        assert_eq!(t.screen(), Screen::Alternate);
    });
}
