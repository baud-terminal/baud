//! **DECHEBM, `CSI ? 35 h/l`** (`[[source: vt510-dechebm]]`): "This
//! control function sets the keyboard mapping to Hebrew/N-A and resets the
//! mapping to North American." Group 2 is the standard Israeli layout by
//! the key's place (`baud_domain::hebrew_key`), and acts while the
//! embedder says the system has a Hebrew layout.

mod support;

use baud_core::*;
use baud_domain::{Codepoint, HebrewLayout, KeyCode, KeyEvent, Modifiers, PackedCell};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

const HEBM: &str = "\x1b[?35h";

fn cp(c: char) -> Codepoint {
    Codepoint::from_char(c)
}

/// DECRQM's answer for private mode `n`.
fn decrqm(t: &mut Term<'_>, n: u16) -> String {
    t.clear_reply();
    support::feed(t, format!("\x1b[?{n}$p").as_bytes());
    String::from_utf8_lossy(t.reply()).into_owned()
}

/// A text key: its identity, what it typed and the US layout's character
/// on it where that differs, as the apps build one.
fn key(identity: char, text: char, us: Option<char>, mods: Modifiers) -> KeyEvent {
    let mut e = KeyEvent::press(KeyCode::Char(cp(identity)), mods);
    e.text = Some(cp(text));
    e.base_layout = us.map(cp);
    e
}

/// What key `e` sends after `setup`, with the system's Hebrew layout as
/// `layout` says.
fn sends(setup: &str, layout: HebrewLayout, e: KeyEvent) -> String {
    support::with_term(&WS, |t| {
        t.set_hebrew_layout(layout);
        support::feed(t, setup.as_bytes());
        t.clear_reply();
        t.key(e);
        String::from_utf8_lossy(t.reply()).into_owned()
    })
}

/// The first `n` characters of row 0.
fn row0(t: &Term<'_>, n: usize) -> String {
    let cells: Vec<PackedCell> = support::screen(t);
    cells[..n]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('.')
        })
        .collect()
}

/// **DECRQM answers DECHEBM**, North American at power-on, Hebrew once
/// `?35 h` sets it; `DECSTR` leaves it, as its table does not list it
/// (`[[source: vt510-decstr]]`); `RIS` returns it; XTSAVE keeps it. Until
/// now `?35` was counted and answered `0`.
#[test]
fn decrqm_answers_dechebm_decstr_keeps_it_and_ris_resets_it() {
    support::with_term(&WS, |t| {
        assert_eq!(decrqm(t, 35), "\x1b[?35;2$y");
        support::feed(t, HEBM.as_bytes());
        assert_eq!(decrqm(t, 35), "\x1b[?35;1$y");
        support::feed(t, b"\x1b[!p");
        assert_eq!(decrqm(t, 35), "\x1b[?35;1$y", "DECSTR");
        support::feed(t, b"\x1b[?35s\x1b[?35l\x1b[?35r");
        assert_eq!(decrqm(t, 35), "\x1b[?35;1$y", "XTRESTORE");
        support::feed(t, b"\x1bc");
        assert_eq!(decrqm(t, 35), "\x1b[?35;2$y", "RIS");
    });
}

/// **Group 2 types the standard Israeli layout's letter on the key's
/// place**: the key where the US layout has `a` sends shin on a US layout
/// and on AZERTY, where it types `q`; `,` sends tav and `q` a slash; a
/// digit is the same in both. With Shift a key types SI-1452's second
/// level: the Latin capitals, and where it differs from the US layout's,
/// the six mirrored keys, `(` and `)`, `{` and `}`, `<` and `>`, each
/// the other (`[[source: xkb-symbols-il-2ec770a]]`, `AE09`, `AE10`,
/// `AD11`, `AD12`, `AB08`, `AB09`). Control is not mapped. Under 8-bit
/// characters, DECHEM set at power-on, the letter is sent as itself.
#[test]
fn group_two_types_the_israeli_layouts_letter_on_the_keys_place() {
    let p = HebrewLayout::Present;
    let none = Modifiers::NONE;
    let shin = "\u{5E9}";
    assert_eq!(sends(HEBM, p, key('a', 'a', None, none)), shin, "US");
    assert_eq!(
        sends(HEBM, p, key('q', 'q', Some('a'), none)),
        shin,
        "AZERTY"
    );
    assert_eq!(sends(HEBM, p, key(',', ',', None, none)), "\u{5EA}", "tav");
    assert_eq!(sends(HEBM, p, key('q', 'q', None, none)), "/");
    assert_eq!(sends(HEBM, p, key('1', '1', None, none)), "1");
    let shift = Modifiers::SHIFT;
    for (identity, text, want) in [
        ('a', 'A', 'A'),
        ('q', 'Q', 'Q'),
        ('9', '(', ')'),
        ('0', ')', '('),
        ('[', '{', '}'),
        (']', '}', '{'),
        (',', '<', '>'),
        ('.', '>', '<'),
        (';', ':', ':'),
        ('\'', '"', '"'),
        ('/', '?', '?'),
        ('`', '~', '~'),
    ] {
        assert_eq!(
            sends(HEBM, p, key(identity, text, None, shift)),
            want.to_string(),
            "Shift+{identity}"
        );
    }
    let mut ctrl = key('a', 'a', None, Modifiers::CTRL);
    ctrl.text = None;
    assert_eq!(sends(HEBM, p, ctrl), "\u{1}", "Control");
    assert_eq!(sends("", p, key('a', 'a', None, none)), "a", "reset");
    assert_eq!(
        sends(&format!("{HEBM}\x1b[?35l"), p, key('a', 'a', None, none)),
        "a",
        "reset again"
    );
}

/// **A shifted key takes SI-1452's second level as the apps send it**:
/// both name the key by what it typed, Shift consumed, with the US
/// layout's character on its place beside it where that differs, as
/// macOS always does. The key where the US layout types `{` sends `}`,
/// and the one where it types `:` sends `:`, not the final pe the key
/// types unshifted; on the system's Hebrew layout, which types SI-1452
/// itself, a shifted key is sent as typed. A key that names no place is
/// sent as typed too: on Wayland a Hebrew layout's Shift+`9` and the ISO
/// key's `<` arrive as a US layout's Shift+`9` and Shift+`,` would.
#[test]
fn a_shifted_key_takes_the_second_level_as_the_apps_send_it() {
    let p = HebrewLayout::Present;
    let none = Modifiers::NONE;
    for (place, text, want) in [
        ('9', '(', ')'),
        ('0', ')', '('),
        ('[', '{', '}'),
        (']', '}', '{'),
        (',', '<', '>'),
        ('.', '>', '<'),
        (';', ':', ':'),
        ('\'', '"', '"'),
        ('/', '?', '?'),
        ('`', '~', '~'),
    ] {
        let us = key(text, text, Some(place), none);
        assert_eq!(sends(HEBM, p, us), want.to_string(), "US {text}");
        let hebrew = key(want, want, Some(place), none);
        assert_eq!(sends(HEBM, p, hebrew), want.to_string(), "Hebrew {want}");
    }
    let unshifted = key('[', '[', None, none);
    assert_eq!(sends(HEBM, p, unshifted), "]", "the first level");
    for c in [')', '<', '('] {
        let no_place = key(c, c, None, none);
        assert_eq!(sends(HEBM, p, no_place), c.to_string(), "no place {c}");
    }
}

/// **Only with a Hebrew keyboard**: "DECHEBM is only valid when a Hebrew
/// keyboard language is selected", so with no Hebrew layout on the system
/// the keys and the screen are as with it reset.
#[test]
fn without_a_hebrew_layout_it_does_nothing() {
    let a = HebrewLayout::Absent;
    let none = Modifiers::NONE;
    assert_eq!(sends(HEBM, a, key('a', 'a', None, none)), "a");
    assert_eq!(
        sends(
            &format!("\x1b[?36l{HEBM}"),
            a,
            key('\u{5E9}', '\u{5E9}', Some('a'), none)
        ),
        "\u{5E9}"
    );
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[?36l\x1b(B{HEBM}abc").as_bytes());
        assert_eq!(row0(t, 3), "abc");
    });
}

/// **Under 7-bit characters**, DECHEM reset, "Makes DEC 7-bit Hebrew the
/// keyboard character set": a letter mapped, and one the system's Hebrew
/// layout typed, is sent as its code, shin at 79, alef at 60.
#[test]
fn under_seven_bit_characters_a_letter_is_sent_as_its_code() {
    let p = HebrewLayout::Present;
    let none = Modifiers::NONE;
    let seven = format!("\x1b[?36l{HEBM}");
    assert_eq!(sends(&seven, p, key('a', 'a', None, none)), "y", "shin");
    assert_eq!(sends(&seven, p, key('t', 't', None, none)), "`", "alef");
    let typed = key('\u{5E9}', '\u{5E9}', Some('a'), none);
    assert_eq!(sends(&seven, p, typed), "y", "typed on the Hebrew layout");
    assert_eq!(sends(HEBM, p, typed), "\u{5E9}", "8-bit: as itself");
    assert_eq!(sends("\x1b[?36l", p, typed), "\u{5E9}", "DECHEBM reset");
}

/// **Under 7-bit characters, ASCII in GL is drawn as DEC 7-bit Hebrew**:
/// "Causes received characters in the range of 60 to 7A (hex) to be
/// displayed using the DEC 7-bit Hebrew when ASCII is designated or
/// invoked and keyboard Group 2 is active"; reset, "displayed using the
/// graphic left (GL) table of characters in use at the time", and "it
/// does not convert ASCII or Hebrew characters already on the display".
/// Under 8-bit characters it does "not affect ... the processing of
/// received characters".
#[test]
fn under_seven_bit_characters_ascii_in_gl_is_drawn_as_hebrew() {
    support::with_term(&WS, |t| {
        t.set_hebrew_layout(HebrewLayout::Present);
        support::feed(t, format!("\x1b[?36l\x1b(Bab{HEBM}`ab").as_bytes());
        assert_eq!(row0(t, 5), "ab\u{5D0}\u{5D1}\u{5D2}");
        support::feed(t, b"\x1b[?35lc{");
        assert_eq!(row0(t, 7), "ab\u{5D0}\u{5D1}\u{5D2}c{");
    });
    support::with_term(&WS, |t| {
        t.set_hebrew_layout(HebrewLayout::Present);
        support::feed(t, format!("{HEBM}ab").as_bytes());
        assert_eq!(row0(t, 2), "ab", "8-bit characters");
    });
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[?36l\x1b(B{HEBM}a").as_bytes());
        t.set_hebrew_layout(HebrewLayout::Present);
        support::feed(t, b"a");
        t.set_hebrew_layout(HebrewLayout::Absent);
        support::feed(t, b"a");
        assert_eq!(row0(t, 3), "a\u{5D1}a", "the embedder's word, each way");
    });
}

/// **Each display draws by DECHEBM as it is when it is written**: set
/// while the status line is written, it draws the main display's ASCII as
/// Hebrew once that is written again.
#[test]
fn the_main_display_draws_by_dechebm_set_on_the_status_line() {
    support::with_term(&WS, |t| {
        t.set_hebrew_layout(HebrewLayout::Present);
        support::feed(t, b"\x1b[?36l\x1b(B\x1b[2$~\x1b[1$}\x1b[?35h\x1b[0$}a");
        assert_eq!(row0(t, 1), "\u{5D1}");
    });
}

/// **At Level 4, the VT510's**: under `DECSCL 61`, VT100 mode, `?35` is
/// counted and not acted on.
#[test]
fn below_level_4_it_is_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[61\"p");
        let before = t.unsupported_param;
        support::feed(t, HEBM.as_bytes());
        assert_eq!(t.unsupported_param, before + 1);
        support::feed(t, b"\x1b[64;1\"p");
        assert_eq!(decrqm(t, 35), "\x1b[?35;2$y");
    });
}
