//! **terminal-wg's three BiDi modes, as the core keeps them**:
//! `CSI ? 2500 h/l` box mirroring, `CSI ? 2501 h/l` direction
//! autodetection and `CSI ? 1243 h/l` arrow-key swapping
//! (`[[source: terminal-wg-bidi-escape-sequences]]`). The first two are
//! paragraph properties every row of a paragraph holds, carried from line
//! to line by the rules SCP's path is carried by; the third is global and
//! read by `Term::key`. What the first two draw is
//! `baud-render/tests/paragraph_modes.rs`.

mod support;

use baud_core::*;
use baud_domain::{Autodetect, BoxMirror};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

const HEBREW: &str = "\u{5D0}\u{5D1}\u{5D2}";

/// DECRQM's answer for private mode `n`.
fn decrqm(t: &mut Term<'_>, n: u16) -> String {
    t.clear_reply();
    support::feed(t, format!("\x1b[?{n}$p").as_bytes());
    String::from_utf8_lossy(t.reply()).into_owned()
}

fn autodetect(t: &Term<'_>) -> Vec<Autodetect> {
    let vp = t.view(Rows::ZERO);
    (0..vp.rows().index())
        .map(|y| vp.bidi(Row::from_index(y)).autodetect())
        .collect()
}

fn mirror(t: &Term<'_>) -> Vec<BoxMirror> {
    let vp = t.view(Rows::ZERO);
    (0..vp.rows().index())
        .map(|y| vp.bidi(Row::from_index(y)).box_mirror())
        .collect()
}

/// What a key sends after `bytes`.
fn sends(bytes: &str, code: KeyCode) -> String {
    support::with_term(&WS, |t| {
        support::feed(t, bytes.as_bytes());
        t.clear_reply();
        assert!(t.key(KeyEvent::press(code, Modifiers::NONE)));
        String::from_utf8_lossy(t.reply()).into_owned()
    })
}

/// **DECRQM answers all three**, each in its power-on state: box mirroring
/// reset, autodetection set (a line nobody set a direction for takes its
/// first strong letter's, the owner's answer of 2026-10-24; Jev's lean,
/// SET_FALLBACK) and arrow-key swapping set ("needs to be the default";
/// Jev's lean, SET), and each as it is set and reset. Until now each was
/// counted and answered `0`.
#[test]
fn decrqm_answers_the_three_modes() {
    support::with_term(&WS, |t| {
        assert_eq!(decrqm(t, 2500), "\x1b[?2500;2$y");
        assert_eq!(decrqm(t, 2501), "\x1b[?2501;1$y");
        assert_eq!(decrqm(t, 1243), "\x1b[?1243;1$y");
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[?2500h\x1b[?2501l\x1b[?1243l");
        assert_eq!(t.unsupported_param, before, "none of them counted");
        assert_eq!(decrqm(t, 2500), "\x1b[?2500;1$y");
        assert_eq!(decrqm(t, 2501), "\x1b[?2501;2$y");
        assert_eq!(decrqm(t, 1243), "\x1b[?1243;2$y");
    });
}

/// **XTSAVE keeps each and XTRESTORE brings it back**: "apart from setting
/// to high or low, there's also a slot for saving and restoring the value",
/// and "Emulators that don't implement saving and restoring should go
/// ahead and implement it".
#[test]
fn xtsave_and_xtrestore_keep_the_three_modes() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2500;2501;1243s\x1b[?2500h\x1b[?2501l\x1b[?1243l");
        support::feed(t, b"\x1b[?2500;2501;1243r");
        assert_eq!(decrqm(t, 2500), "\x1b[?2500;2$y");
        assert_eq!(decrqm(t, 2501), "\x1b[?2501;1$y");
        assert_eq!(decrqm(t, 1243), "\x1b[?1243;1$y");
    });
}

/// **A mode applies at once only to the paragraph whose first position the
/// cursor is on** (terminal-wg's draft, "if the cursor stands at the first
/// logical position of a paragraph then the received BiDi property value
/// is immediately applied on that paragraph"; Jev, FIRST_POSITION,
/// 0.90/0.94): sent mid-line, or at column 1 of a row a wrap brought in,
/// it changes only the mode in force, which the next line a line feed
/// reaches takes.
#[test]
fn a_mode_applies_to_the_paragraph_whose_start_the_cursor_is_on() {
    support::with_term(&WS, |t| {
        support::feed(t, b"ab\x1b[?2501l");
        assert_eq!(autodetect(t)[0], Autodetect::On, "sent mid-line");
        support::feed(t, b"\r\x1b[?2501l");
        assert_eq!(autodetect(t)[0], Autodetect::Off, "sent at its start");
        support::feed(t, b"\x1b[?2501h\x1b[?2500h\r\ncdefghijklmn");
        assert_eq!(
            autodetect(t)[1],
            Autodetect::On,
            "a new line takes the mode in force"
        );
        assert_eq!(
            mirror(t)[1..3],
            [BoxMirror::On, BoxMirror::On],
            "and its wrapped row"
        );
        support::feed(t, b"\x1b[3;1H\x1b[?2500l");
        assert_eq!(
            mirror(t)[1..3],
            [BoxMirror::On, BoxMirror::On],
            "column 1 of a wrapped row is not a paragraph's start"
        );
        support::feed(t, b"\x1b[2;1H\x1b[?2500l");
        assert_eq!(
            mirror(t)[1..3],
            [BoxMirror::Off, BoxMirror::Off],
            "its first row is"
        );
    });
}

/// **DECSTR returns the modes in force to their power-on states and leaves
/// the lines'; RIS returns every line too** (Jev's lean, RESET_ALL): as
/// mintty's soft reset returns its paragraph modes, VTE's every mode, and
/// DEC's table the cursor keys.
#[test]
fn decstr_returns_the_modes_in_force_and_ris_every_line() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2501l\x1b[?2500h\x1b[?1243l");
        support::feed(t, b"\x1b[!p");
        assert_eq!(decrqm(t, 2500), "\x1b[?2500;2$y");
        assert_eq!(decrqm(t, 2501), "\x1b[?2501;1$y");
        assert_eq!(decrqm(t, 1243), "\x1b[?1243;1$y");
        assert_eq!(autodetect(t)[0], Autodetect::Off, "the line keeps its own");
        support::feed(t, b"\r\n");
        assert_eq!(
            autodetect(t)[1],
            Autodetect::On,
            "the next takes power-on's"
        );
        support::feed(t, b"\x1bc");
        assert_eq!(autodetect(t), [Autodetect::On; 4]);
        assert_eq!(mirror(t), [BoxMirror::Off; 4]);
    });
}

/// **With autodetection on a path is only the fallback** (terminal-wg:
/// "The sequence affects ... the fallback direction of implicit paragraphs
/// with autodetection"; Jev's lean, SET_FALLBACK): SCP 2 leaves an English
/// line left to right and puts a line of digits right to left; with it off
/// SCP 2 turns the English line, and an unset Hebrew line runs left to
/// right.
#[test]
fn a_path_is_the_fallback_while_autodetection_is_on() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2 kabc\r\n123\r\nab\x1b[?2501lc");
        let dir = |t: &Term<'_>, y| t.paragraph_direction(Row::from_index(y), Col::ZERO);
        assert_eq!(dir(t, 0), BidiDirection::Ltr, "English, first strong");
        assert_eq!(dir(t, 1), BidiDirection::Rtl, "digits, the fallback");
        assert_eq!(
            dir(t, 2),
            BidiDirection::Ltr,
            "sent mid-way: applies to the next line"
        );
        support::feed(t, b"\r\nabc");
        assert_eq!(dir(t, 3), BidiDirection::Rtl, "the path outright");
    });
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[?2501l{HEBREW}").as_bytes());
        assert_eq!(
            t.paragraph_direction(Row::ZERO, Col::ZERO),
            BidiDirection::Ltr,
            "no path and no detection: left to right"
        );
    });
}

/// **Left and Right swap inside a right-to-left paragraph** (terminal-wg:
/// "the sequences generated by the Left and Right arrows are swapped
/// whenever the cursor is inside a RTL paragraph"), from power-on, the
/// keypad's too, under DECCKM and kitty's protocol; on a left-to-right
/// line, and with `?1243` reset, they are as pressed. Home and End never
/// swap.
#[test]
fn the_arrow_keys_swap_inside_a_right_to_left_paragraph() {
    assert_eq!(sends(HEBREW, KeyCode::Left), "\x1b[C");
    assert_eq!(sends(HEBREW, KeyCode::Right), "\x1b[D");
    assert_eq!(
        sends(HEBREW, KeyCode::Keypad(KeypadKey::Left)),
        "\x1b[C",
        "the keypad's"
    );
    assert_eq!(
        sends(&format!("\x1b[?1h{HEBREW}"), KeyCode::Left),
        "\x1bOC",
        "DECCKM"
    );
    assert_eq!(sends("abc", KeyCode::Left), "\x1b[D", "the control");
    assert_eq!(
        sends(&format!("\x1b[?1243l{HEBREW}"), KeyCode::Left),
        "\x1b[D"
    );
    assert_eq!(sends(HEBREW, KeyCode::Home), "\x1b[H", "Home");
    assert_eq!(sends(HEBREW, KeyCode::End), "\x1b[F", "End");
    assert_eq!(sends(HEBREW, KeyCode::Up), "\x1b[A");
    // Under kitty's protocol, every key an escape code, the swapped key is
    // the other key's code in that encoding.
    let kitty = "\x1b[>8u";
    assert_eq!(
        sends(&format!("{kitty}{HEBREW}"), KeyCode::Left),
        sends(&format!("{kitty}abc"), KeyCode::Right),
        "kitty's encoding"
    );
    assert_ne!(
        sends(&format!("{kitty}abc"), KeyCode::Right),
        sends(&format!("{kitty}abc"), KeyCode::Left),
        "the control"
    );
}

/// **The press decides, and its repeats and release name its key**: the
/// draft swaps "conditionally to the cursor being inside an RTL paragraph
/// at the time the key is pressed" (`[[source:
/// terminal-wg-bidi-escape-sequences]]`). Pressed in Hebrew, then moved to
/// an English line or with `?1243` reset, Left's repeat and release still
/// send Right, under kitty's event types and on the base layer, where a
/// repeat is sent as a press; a release forgets the decision, as `RIS`
/// and `DECSTR` do, and a repeat or release with no press recorded goes
/// as pressed.
#[test]
fn a_repeat_and_a_release_take_the_decision_of_the_press() {
    let left = KeyEvent::press(KeyCode::Left, Modifiers::NONE);
    let repeat = KeyEvent {
        kind: KeyEventKind::Repeat,
        ..left
    };
    let release = KeyEvent {
        kind: KeyEventKind::Release,
        ..left
    };
    let key = |t: &mut Term<'_>, e: KeyEvent| {
        t.clear_reply();
        t.key(e);
        String::from_utf8_lossy(t.reply()).into_owned()
    };
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[>10u{HEBREW}").as_bytes());
        assert_eq!(key(t, left), "\x1b[C", "the press, swapped");
        support::feed(t, b"\r\nabc");
        assert_eq!(key(t, repeat), "\x1b[1;1:2C", "its repeat");
        assert_eq!(key(t, release), "\x1b[1;1:3C", "its release");
        assert_eq!(key(t, left), "\x1b[D", "the control: a press here");
        assert_eq!(key(t, release), "\x1b[1;1:3D");
    });
    support::with_term(&WS, |t| {
        support::feed(t, HEBREW.as_bytes());
        assert_eq!(key(t, left), "\x1b[C");
        support::feed(t, b"\r\nabc");
        assert_eq!(key(t, repeat), "\x1b[C", "the base layer's repeat");
        support::feed(t, format!("\x1b[?1243l\x1b[1;4H{HEBREW}").as_bytes());
        assert_eq!(key(t, repeat), "\x1b[C", "?1243 reset since");
        assert_eq!(key(t, left), "\x1b[D", "the control: a press now");
    });
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[>10u{HEBREW}").as_bytes());
        assert_eq!(key(t, release), "\x1b[1;1:3D", "no press recorded");
        assert_eq!(key(t, left), "\x1b[C");
        assert_eq!(key(t, release), "\x1b[1;1:3C");
        assert_eq!(key(t, repeat), "\x1b[1;1:2D", "the release forgot it");
    });
    // `RIS` and `DECSTR` forget a held key's decision: its repeat on an
    // English line after either goes as pressed.
    for reset in ["\x1bc", "\x1b[!p\r\n"] {
        support::with_term(&WS, |t| {
            support::feed(t, HEBREW.as_bytes());
            assert_eq!(key(t, left), "\x1b[C");
            support::feed(t, format!("{reset}abc").as_bytes());
            assert_eq!(key(t, repeat), "\x1b[D", "after {reset:?}");
        });
    }
}

/// **The direction is the autodetected one, not the fallback**, and the
/// direction a path sets where nothing is detected: SCP 2 swaps on an
/// empty line and on one of digits, not on an English one; with BDSM
/// reset, only a line SCP 2 set right to left swaps.
#[test]
fn the_swap_follows_the_direction_drawn() {
    assert_eq!(sends("\x1b[2 kabc", KeyCode::Left), "\x1b[D", "English");
    assert_eq!(sends("\x1b[2 k123", KeyCode::Left), "\x1b[C", "digits");
    assert_eq!(sends("\x1b[2 k", KeyCode::Left), "\x1b[C", "empty");
    assert_eq!(
        sends(&format!("\x1b[8l{HEBREW}"), KeyCode::Left),
        "\x1b[D",
        "explicit: the stored order"
    );
    assert_eq!(sends("\x1b[8l\x1b[2 kabc", KeyCode::Left), "\x1b[C");
}

/// **On a table row the cell holds the direction**: a row holding a Box
/// Drawing character is laid out cell by cell, each cell a paragraph of
/// its own (UAX #9 section 3; Jev, CELL, 0.98/0.98), so the cursor in a
/// Hebrew cell swaps and in a Latin one does not; on a rule it is in the
/// cell before it, and past the row's text in none.
#[test]
fn on_a_table_row_the_cell_holds_the_direction() {
    let row = format!("\u{2502}{HEBREW}\u{2502}ab\u{2502}");
    let at = |c: u16| format!("{row}\x1b[1;{c}H");
    assert_eq!(sends(&at(3), KeyCode::Left), "\x1b[C", "in the Hebrew cell");
    assert_eq!(sends(&at(6), KeyCode::Left), "\x1b[D", "in the Latin cell");
    assert_eq!(
        sends(&at(5), KeyCode::Left),
        "\x1b[C",
        "on the rule after the Hebrew"
    );
    let open = format!("\u{2502}ab\u{2502}{HEBREW}");
    assert_eq!(
        sends(&open, KeyCode::Left),
        "\x1b[D",
        "past the row's text, in no cell, drawn left to right"
    );
}

/// **A paragraph that begins in the history is read from its start**: its
/// first row scrolled off holds the Hebrew letter that makes it right to
/// left.
#[test]
fn a_paragraph_begun_in_the_history_is_read_from_its_start() {
    let text = format!("{HEBREW}abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHI");
    assert_eq!(sends(&text, KeyCode::Left), "\x1b[C");
    assert_eq!(
        sends(&text[HEBREW.len()..], KeyCode::Left),
        "\x1b[D",
        "the control"
    );
}

/// **The wheel's and a click's cursor keys do not swap**: they count the
/// columns a program moves its point by, in its own order.
#[test]
fn the_cursor_keys_the_wheel_and_a_click_send_do_not_swap() {
    support::with_term(&WS, |t| {
        support::feed(t, HEBREW.as_bytes());
        t.clear_reply();
        assert!(t.wheel_key(KeyEvent::press(KeyCode::Left, Modifiers::NONE)));
        assert_eq!(t.reply(), b"\x1b[D");
    });
}

/// **Which way a row is drawn, for what is anchored on it**
/// (`Term::line_direction`): a Hebrew line right to left and a Latin one
/// left to right, a table row left to right whatever its cells hold (it
/// keeps its columns), a right-to-left path with BDSM reset right to left,
/// and a row of a window looking back read in that window.
#[test]
fn a_row_is_drawn_the_way_its_line_runs_and_a_table_row_left_to_right() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            format!("{HEBREW}\r\nabc\r\n{HEBREW}\u{2502}cd").as_bytes(),
        );
        let dir = |t: &Term<'_>, y| t.line_direction(Rows::ZERO, Row::from_index(y));
        assert_eq!(dir(t, 0), BidiDirection::Rtl);
        assert_eq!(dir(t, 1), BidiDirection::Ltr);
        assert_eq!(dir(t, 2), BidiDirection::Ltr, "a table row");
        assert_eq!(
            t.paragraph_direction(Row::from_index(2), Col::ZERO),
            BidiDirection::Rtl,
            "the control: the cell runs right to left"
        );
        // Six lines scrolled into the history: the Hebrew line is row 0
        // of the window six lines back.
        support::feed(t, b"\x1b[99B\n\n\n\n\n\n");
        let back = Rows::of_count(6);
        assert_eq!(t.looking_back(back), back, "the premise: six lines back");
        assert_eq!(t.line_direction(back, Row::ZERO), BidiDirection::Rtl);
        assert_eq!(
            t.line_direction(back, Row::from_index(1)),
            BidiDirection::Ltr
        );
    });
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[8l\x1b[2 kabc\r\n\x1b[1 kdef");
        let dir = |t: &Term<'_>, y| t.line_direction(Rows::ZERO, Row::from_index(y));
        assert_eq!(dir(t, 0), BidiDirection::Rtl, "BDSM reset, the path");
        assert_eq!(dir(t, 1), BidiDirection::Ltr);
    });
}
