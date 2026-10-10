#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! The base-layer key encoder, checked against `[[source: ctlseqs]]`.
//!
//! `docs/input-coverage.md` names three layers and this is the one every
//! terminal implements. An application that has not negotiated an enhancement
//! must receive what any other terminal would send, which is the same reasoning
//! that makes `DA` under-claim as a VT102.
//!
//! The expectations are read out of the pinned specification rather than
//! written here, for the reason `scripts/extract-keys.py` exists: a key table
//! transcribed by hand is a guessed table that looks like it worked.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

fn ctlseqs() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs")
}

/// Whether a line is a table rule rather than a row.
///
/// All rule characters, not merely starting with one: `- (minus)` is a key.
fn is_rule(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c == '-' || c == '+' || c.is_whitespace())
}

/// The `Key | Normal | Application` table at the given line, as
/// `(name, normal, application)`.
fn table_at(src: &str, line: usize) -> Vec<(String, String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        // A separator is *all* rule characters. Testing `starts_with('-')` --
        // which this did -- also matches the `- (minus)` key, so the keypad
        // table silently arrived one row short and the count assertion is what
        // caught it. A row dropped by the reader looks exactly like a row the
        // specification does not have.
        if is_rule(t) {
            continue;
        }
        let Some((name, rest)) = t.split_once('|') else {
            break;
        };
        let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
        if cells.len() < 2 {
            break;
        }
        out.push((
            name.trim().to_string(),
            cells[0].to_string(),
            cells[1].to_string(),
        ));
    }
    out
}

fn press(code: KeyCode, mods: Modifiers, app: bool) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if app {
        // DECCKM set: cursor keys go application.
        let b = b"\x1b[?1h";
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
        t.clear_reply();
    }
    assert!(t.key(KeyEvent::press(code, mods)), "the key was encoded");
    t.reply().to_vec()
}

/// Turn the specification's spelling -- `CSI A`, `SS3 A` -- into bytes.
fn spec_bytes(form: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for tok in form.split_whitespace() {
        match tok {
            "CSI" => out.extend_from_slice(b"\x1b["),
            "SS3" => out.extend_from_slice(b"\x1bO"),
            // VT52 spells its introducers with a bare `ESC`, and without this
            // arm the expectation for `ESC ? SP` was the three letters E, S, C.
            "ESC" => out.push(0x1b),
            // The specification names three bytes rather than printing them,
            // and they appear *inside* a form: the keypad's Space is
            // `SS3 SP`, which is three bytes and not four.
            "SP" => out.push(b' '),
            "TAB" => out.push(0x09),
            "CR" => out.push(0x0d),
            other => out.extend_from_slice(other.as_bytes()),
        }
    }
    out
}

/// **Every cursor key matches the specification's table, in both modes.**
///
/// Six keys across two tables -- the four arrows at line 2473 and Home/End at
/// 2485 -- read from the source and compared against what baud sends.
#[test]
fn cursor_keys_match_the_specifications_table() {
    let src = ctlseqs();
    let rows: Vec<(String, String, String)> = table_at(&src, 2473)
        .into_iter()
        .chain(table_at(&src, 2485))
        .collect();
    assert_eq!(
        rows.len(),
        6,
        "expected four arrows and Home/End from ctlseqs; got {rows:?}. If the \
         source was re-pinned the line numbers moved, and an empty table would \
         pass every assertion below by having nothing to check."
    );

    for (name, normal, app) in rows {
        let code = match name.as_str() {
            "Cursor Up" => KeyCode::Up,
            "Cursor Down" => KeyCode::Down,
            "Cursor Right" => KeyCode::Right,
            "Cursor Left" => KeyCode::Left,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            other => panic!("unknown key {other:?} in the table"),
        };
        assert_eq!(
            press(code, Modifiers::NONE, false),
            spec_bytes(&normal),
            "{name} in normal mode should be {normal}"
        );
        assert_eq!(
            press(code, Modifiers::NONE, true),
            spec_bytes(&app),
            "{name} in application mode should be {app}"
        );
    }
}

/// **A modifier is appended as `1 ; mod`, and forces `CSI`.**
///
/// `SS3` takes no parameters, so there is nowhere to put the modifier. The
/// specification states this for F1-F4 at line 2564 -- "the SS3 sent before F1
/// through F4 is altered to CSI when sending a function key modifier" -- and
/// the same holds for the cursor keys. An encoder that kept `SS3` in
/// application mode would emit something no application parses.
#[test]
fn a_modified_cursor_key_uses_csi_even_in_application_mode() {
    assert_eq!(
        press(KeyCode::Up, Modifiers::SHIFT, false),
        b"\x1b[1;2A".to_vec(),
        "shift-Up is CSI 1 ; 2 A"
    );
    assert_eq!(
        press(KeyCode::Up, Modifiers::SHIFT, true),
        b"\x1b[1;2A".to_vec(),
        "and application mode does not change that -- SS3 has no parameters"
    );
    assert_eq!(
        press(KeyCode::Left, Modifiers::CTRL.with(Modifiers::SHIFT), false),
        b"\x1b[1;6D".to_vec(),
        "ctlseqs code 6 is Shift + Control"
    );
}

/// **DECARM reset stops a held key repeating, and a fresh terminal repeats.**
/// `[[source: vt510-decarm]]`: "Reset: keys do not autorepeat", and "Default:
/// Repeat (set)". So `?8l` drops a repeat and still sends a press, on the
/// base layer and under kitty's event types alike; `?8h` and a full reset
/// restore it. A soft reset leaves it: DEC's table for DECSTR does not list
/// DECARM, and xterm's soft reset does not touch it.
#[test]
fn a_repeat_is_dropped_while_autorepeat_is_reset() {
    let sent = |setup: &[u8], e: KeyEvent| -> Vec<u8> {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        feed(&mut t, setup);
        t.clear_reply();
        t.key(e);
        t.reply().to_vec()
    };
    let a = KeyEvent::press(KeyCode::Char(Codepoint::from_char('a')), Modifiers::NONE);
    let repeat = KeyEvent {
        kind: KeyEventKind::Repeat,
        ..a
    };
    assert_eq!(sent(b"", repeat), b"a", "a fresh terminal repeats");
    assert!(sent(b"\x1b[?8l", repeat).is_empty(), "?8l stops the repeat");
    assert_eq!(sent(b"\x1b[?8l", a), b"a", "and a press still types");
    assert_eq!(sent(b"\x1b[?8l\x1b[?8h", repeat), b"a", "?8h restores it");
    assert_eq!(sent(b"\x1b[?8l\x1bc", repeat), b"a", "RIS restores it");
    assert!(
        sent(b"\x1b[?8l\x1b[!p", repeat).is_empty(),
        "DECSTR leaves it reset"
    );
    let up = KeyEvent {
        kind: KeyEventKind::Repeat,
        ..KeyEvent::press(KeyCode::Up, Modifiers::NONE)
    };
    assert_eq!(sent(b"\x1b[=3u", up), b"\x1b[1;1:2A", "kitty reports it");
    assert!(
        sent(b"\x1b[=3u\x1b[?8l", up).is_empty(),
        "and reports none while it is reset"
    );
}

/// **A repeat is a press on this layer, and a release is nothing.**
///
/// `[[source: kitty-keyboard]]`: "Normally only key press events are reported
/// and key repeat events are treated as key press events." A held key on a
/// terminal that reports no event types is the key pressed again, which is
/// what every terminal sends while a key is held. A release is the event-types
/// enhancement's alone. This test used to pin the opposite -- a repeat sent
/// nothing, as "bytes nothing requested" -- which would have made holding a
/// key type it once the day an embedder delivered a repeat. None did, so the
/// rule had never run.
#[test]
fn a_repeat_is_a_press_and_a_release_is_nothing() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for code in [
        KeyCode::Up,
        KeyCode::Enter,
        KeyCode::Char(Codepoint::from_char('a')),
    ] {
        let press = KeyEvent::press(code, Modifiers::NONE);
        assert!(t.key(press), "{code:?} pressed");
        let pressed = t.reply().to_vec();
        t.clear_reply();
        let repeat = KeyEvent {
            kind: KeyEventKind::Repeat,
            ..press
        };
        assert!(t.key(repeat), "{code:?} repeated is sent");
        assert_eq!(t.reply(), &pressed[..], "{code:?} repeated is the press");
        t.clear_reply();
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..press
        };
        assert!(!t.key(release), "{code:?} released is not encoded");
        assert!(t.reply().is_empty(), "and writes nothing");
    }
}

/// The `Key | Escape Sequence` table at the given line.
fn seq_table_at(src: &str, line: usize) -> Vec<(String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        if is_rule(t) {
            continue;
        }
        let Some((name, rest)) = t.split_once('|') else {
            break;
        };
        out.push((name.trim().to_string(), rest.trim().to_string()));
    }
    out
}

/// **Every function key matches the specification, gaps included.**
///
/// F5-F12 are 15, 17, 18, 19, 20, 21, 23, 24 and F13-F20 are 25, 26, 28, 29,
/// 31, 32, 33, 34. Sixteen and twenty-two are skipped, and so are twenty-seven
/// and thirty. A table written as "15 upward" is wrong from F6 onward and
/// sends a *neighbouring key* rather than nothing — the failure mode that is
/// hardest to notice, because something plausible arrives.
#[test]
fn function_keys_match_the_specifications_tables() {
    let src = ctlseqs();
    let rows: Vec<(String, String)> = seq_table_at(&src, 2529)
        .into_iter()
        .chain(seq_table_at(&src, 2685))
        .collect();
    assert_eq!(
        rows.len(),
        20,
        "expected F1-F12 and F13-F20 from ctlseqs; got {} rows. An empty table \
         would pass every comparison below by having nothing to compare.",
        rows.len()
    );

    for (name, form) in rows {
        let n: u8 = name.trim_start_matches('F').parse().expect("Fn");
        let key = KeyCode::Function(FnKey::new(n).expect("F1..F20"));
        assert_eq!(
            press(key, Modifiers::NONE, false),
            spec_bytes(&form),
            "F{n} should be {form}"
        );
    }
}

/// The editing keys, and that DECCKM does not touch them.
///
/// `[[source: ctlseqs]]` line 2672 gives the same sequence in the Normal and
/// Application columns, unlike the cursor keys two tables earlier.
#[test]
fn editing_keys_are_the_same_in_both_modes() {
    let src = ctlseqs();
    let rows = table_at(&src, 2672);
    assert_eq!(rows.len(), 6, "Insert, Delete, Home, End, PageUp, PageDown");

    for (name, normal, app) in rows {
        assert_eq!(
            normal, app,
            "{name}: DECCKM does not change the editing keys"
        );
        // Home and End are sent PC-style, from the table at 2485; the VT220
        // forms here are the `sunKeyboard` alternative, not an addition.
        let code = match name.as_str() {
            "Insert" => KeyCode::Insert,
            "Delete" => KeyCode::Delete,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Home" | "End" => continue,
            other => panic!("unknown editing key {other:?}"),
        };
        for app_mode in [false, true] {
            assert_eq!(
                press(code, Modifiers::NONE, app_mode),
                spec_bytes(&normal),
                "{name} should be {normal} in both modes"
            );
        }
    }
}

/// A modifier goes before the `~`, not after it.
///
/// `CSI 1 5 ; 2 ~` is shift-F5, which is the worked example the specification
/// gives at line 2586 for the modifier encoding.
#[test]
fn shift_f5_is_the_specifications_worked_example() {
    let src = ctlseqs();
    assert!(
        src.contains("shift-F5 would be sent as CSI 1 5 ; 2 ~"),
        "the worked example moved; this test quotes a source it no longer matches"
    );
    let f5 = KeyCode::Function(FnKey::new(5).unwrap());
    assert_eq!(press(f5, Modifiers::SHIFT, false), b"\x1b[15;2~".to_vec());

    // And F1-F4 take the cursor keys' shape instead.
    let f1 = KeyCode::Function(FnKey::new(1).unwrap());
    assert_eq!(press(f1, Modifiers::NONE, false), b"\x1bOP".to_vec());
    assert_eq!(press(f1, Modifiers::CTRL, false), b"\x1b[1;5P".to_vec());
}

fn press_raw(code: KeyCode, mods: Modifiers) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    assert!(t.key(KeyEvent::press(code, mods)), "encoded");
    t.reply().to_vec()
}

fn ch(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

/// **Text keys send their bytes, and Control folds them.**
///
/// `@` through `_` and the lowercase letters differ from their control code by
/// bit 6 alone, which is why Ctrl-A is 1 and Ctrl-[ is escape.
#[test]
fn control_folds_a_character_into_its_control_code() {
    assert_eq!(press_raw(ch('a'), Modifiers::NONE), b"a".to_vec());
    assert_eq!(press_raw(ch('a'), Modifiers::CTRL), vec![0x01]);
    assert_eq!(press_raw(ch('A'), Modifiers::CTRL), vec![0x01]);
    assert_eq!(
        press_raw(ch('['), Modifiers::CTRL),
        vec![0x1b],
        "Ctrl-[ is ESC"
    );
    assert_eq!(
        press_raw(ch('@'), Modifiers::CTRL),
        vec![0x00],
        "Ctrl-@ is NUL"
    );
    assert_eq!(press_raw(ch(' '), Modifiers::CTRL), vec![0x00]);
    assert_eq!(press_raw(ch('?'), Modifiers::CTRL), vec![0x7f]);

    // No control form: Control is dropped and the character is sent as itself.
    // The application cannot tell, which is the ambiguity `fixterms` exists to
    // remove and the reason `CSI u` is a layer rather than a nicety.
    assert_eq!(press_raw(ch('1'), Modifiers::CTRL), b"1".to_vec());
}

/// Non-ASCII text is sent as UTF-8, not as a byte.
#[test]
fn a_non_ascii_character_is_sent_as_utf8() {
    assert_eq!(press_raw(ch('é'), Modifiers::NONE), "é".as_bytes().to_vec());
    assert_eq!(
        press_raw(ch('世'), Modifiers::NONE),
        "世".as_bytes().to_vec()
    );
    assert_eq!(
        press_raw(ch('🚀'), Modifiers::NONE),
        "🚀".as_bytes().to_vec()
    );
}

/// **Alt prefixes ESC rather than setting the high bit.**
///
/// `[[source: ctlseqs]]` line 2325 gives both and line 2337 tabulates them;
/// `?1039` chooses for Alt. The prefix is the only one that survives UTF-8:
/// adding 128 to a byte produces a continuation byte, not a character. baud
/// defaults it on after `[[source: foot-meta-defaults]]`, ctlseqs stating no
/// default for the resource.
#[test]
fn alt_prefixes_escape() {
    assert_eq!(press_raw(ch('x'), Modifiers::ALT), vec![0x1b, b'x']);
    assert_eq!(
        press_raw(ch('x'), Modifiers::ALT.with(Modifiers::CTRL)),
        vec![0x1b, 0x18],
        "and it composes with Control"
    );
    assert_eq!(
        press_raw(ch('é'), Modifiers::ALT),
        {
            let mut v = vec![0x1b];
            v.extend_from_slice("é".as_bytes());
            v
        },
        "the prefix goes before the whole UTF-8 sequence, not each byte"
    );
}

/// Enter, Tab, Backspace and Escape.
#[test]
fn the_remaining_text_keys() {
    assert_eq!(press_raw(KeyCode::Enter, Modifiers::NONE), vec![0x0d]);
    assert_eq!(press_raw(KeyCode::Tab, Modifiers::NONE), vec![0x09]);
    assert_eq!(press_raw(KeyCode::Escape, Modifiers::NONE), vec![0x1b]);
    // xterm's default with `backarrowKey` false. DECBKM (`?67`) would choose,
    // and baud does not implement it.
    assert_eq!(press_raw(KeyCode::Backspace, Modifiers::NONE), vec![0x7f]);
    // Shift-Tab is CBT, the one text key that is a sequence.
    assert_eq!(
        press_raw(KeyCode::Tab, Modifiers::SHIFT),
        b"\x1b[Z".to_vec()
    );
}

/// **LNM makes Return send CRLF.**
///
/// The mode governs what LF does on output; on input it is what Return sends,
/// and they are the same question from either end.
#[test]
fn line_feed_new_line_mode_makes_return_send_crlf() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[20h");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"\x1b[20h").len()));
    t.clear_reply();

    assert!(t.key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE)));
    assert_eq!(t.reply(), &[0x0d, 0x0a], "LNM set: CR LF");
}

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
}

/// **`?1036` and `?1039` are two modes, and were one bit under a third number.**
///
/// `[[source: ctlseqs]]` line 995 gives `?1034` as `eightBitInput` -- add 128 --
/// and line 1000 gives `?1036` as `metaSendsEscape` -- prefix `ESC`. baud
/// implemented the second under the first's number, which nothing could
/// detect while no encoder read the bit.
///
/// Four of the sixteen rows at line 2337 disagree between the two columns, so
/// one bit cannot answer both: turning Alt's prefix off must leave Meta's on.
#[test]
fn alt_and_meta_prefixes_are_separate_modes() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    assert!(
        t.alt_sends_escape() && t.meta_sends_escape(),
        "both on by default"
    );

    feed(&mut t, b"\x1b[?1039l");
    t.clear_reply();
    assert!(!t.alt_sends_escape(), "?1039 l turned Alt off");
    assert!(t.meta_sends_escape(), "and left Meta alone");

    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::ALT)));
    assert_eq!(t.reply(), b"x", "Alt no longer prefixes");
    t.clear_reply();
    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::META)));
    assert_eq!(t.reply(), b"\x1bx", "Meta still does");

    feed(&mut t, b"\x1b[?1036l");
    t.clear_reply();
    assert!(!t.meta_sends_escape());
    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::META)));
    assert_eq!(
        t.reply(),
        b"x",
        "with the prefix off and `?1034` unimplemented, the table's `shift` \
         column has no implementation, so the bare character is what is left"
    );
}

/// Both modifiers held prefix one `ESC`, not two.
///
/// `ESC ESC x` is Escape followed by Alt-x, which is a different keystroke.
#[test]
fn alt_and_meta_together_prefix_one_escape() {
    assert_eq!(
        press_raw(ch('x'), Modifiers::ALT.with(Modifiers::META)),
        vec![0x1b, b'x']
    );
}

/// **All three modes exist and DECRQM tells them apart.**
///
/// They were one bit under one number, so a DECRQM for any of the three
/// answered about whichever the bit happened to be. `?1034` and `?1036` are
/// opposite branches of the same table, and an application asking which is in
/// force got the same answer for both.
#[test]
fn all_three_meta_modes_report_separately() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b[?1034$p\x1b[?1036$p\x1b[?1039$p");
    assert_eq!(
        t.reply(),
        b"\x1b[?1034;2$y\x1b[?1036;1$y\x1b[?1039;1$y",
        "eightBitInput reset, both prefixes set"
    );
    t.clear_reply();

    // Reset means "exists and is off". `0` would say it does not exist, and an
    // application would then set it and believe it took effect -- which is
    // exactly what `?1036` and `?1039` did before they had numbers of their own.
    feed(&mut t, b"\x1b[?1034h\x1b[?1039l");
    t.clear_reply();
    feed(&mut t, b"\x1b[?1034$p\x1b[?1039$p");
    assert_eq!(t.reply(), b"\x1b[?1034;1$y\x1b[?1039;2$y");
}

/// **`?1034` is the terminfo one, and it is encoding-conditional.**
///
/// `smm` and `rmm` are `CSI ? 1034 h` / `l` in every `xterm*` entry, which is
/// why this is implemented rather than refused: leaving it unrecognised left
/// two capabilities of the corpus dropped, and `tests/conformance` counted
/// them.
///
/// Setting it under UTF-8 changes nothing, and that is not a mode set and
/// never read: `[[source: ctlseqs]]` line 2331 shifts a key "from codes 0-127
/// to 128-255", and 128-255 under UTF-8 is the continuation range. The mode is
/// recorded and DECRQM reports it truthfully in both encodings; the encoding
/// decides whether the shift applies.
#[test]
fn eight_bit_input_applies_only_under_latin1() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    assert!(
        !t.eight_bit_input(),
        "reset by default -- the ESC prefix is baud's"
    );

    // Turn the prefix off so the other branch of the table is reachable at all,
    // then ask for the shift.
    feed(&mut t, b"\x1b[?1039l\x1b[?1036l\x1b[?1034h");
    t.clear_reply();
    assert!(t.eight_bit_input());
    assert_eq!(t.input_encoding(), InputEncoding::Utf8);

    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::ALT)));
    assert_eq!(
        t.reply(),
        b"x",
        "UTF-8: no shift, because 0xF8 is not a character"
    );
    t.clear_reply();

    // `ESC % @` selects ISO 8859-1, where a byte above 127 *is* a character.
    feed(&mut t, b"\x1b%@");
    t.clear_reply();
    assert_eq!(t.input_encoding(), InputEncoding::Latin1);

    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::ALT)));
    assert_eq!(t.reply(), &[0xf8], "Latin-1: 'x' | 0x80");
    t.clear_reply();

    // Control folds first, then the shift: `Meta-Ctrl-A` is `0x81`, a C1
    // control, which is what baud accepts under this very encoding.
    assert!(t.key(KeyEvent::press(
        ch('a'),
        Modifiers::ALT.with(Modifiers::CTRL)
    )));
    assert_eq!(t.reply(), &[0x81]);
    t.clear_reply();

    // The prefix wins where both are set. `[[source: ctlseqs]]` line 2352: with
    // both resources ON every modified row is `ESC x`, and none is shifted.
    feed(&mut t, b"\x1b[?1039h");
    t.clear_reply();
    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::ALT)));
    assert_eq!(t.reply(), b"\x1bx");
    t.clear_reply();

    // **And it stops when the encoding goes back**, with the mode still set.
    // The condition is read per keystroke, not latched when `?1034 h` arrived.
    // Same rule as `?1020` mirroring the live encoding rather than the one in
    // force when it was asked: a mode that outlives the fact it depends on is
    // the failure the mode table exists to prevent.
    feed(&mut t, b"\x1b[?1039l\x1b%G");
    t.clear_reply();
    assert!(t.eight_bit_input(), "the mode is still set");
    assert_eq!(t.input_encoding(), InputEncoding::Utf8);
    assert!(t.key(KeyEvent::press(ch('x'), Modifiers::ALT)));
    assert_eq!(t.reply(), b"x", "and no longer applies");
}

/// **Under ISO 8859-1 the keyboard sends ISO 8859-1.**
///
/// `ESC % @` switches how baud decodes, and an encoder that kept emitting
/// UTF-8 would break the round trip that makes the mode useful — the
/// application set the encoding and would read `é` back as two characters.
///
/// Above `U+00FF` there is no byte and no key on such a keyboard, so nothing
/// is sent rather than UTF-8 the application would misread.
#[test]
fn latin1_input_is_encoded_as_latin1() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    feed(&mut t, b"\x1b%@");
    t.clear_reply();

    assert!(t.key(KeyEvent::press(ch('é'), Modifiers::NONE)));
    assert_eq!(t.reply(), &[0xe9], "one byte, not the two UTF-8 would give");
    t.clear_reply();

    assert!(
        !t.key(KeyEvent::press(ch('世'), Modifiers::NONE)),
        "no Latin-1 byte exists for it"
    );
    assert!(t.reply().is_empty());

    // Back to UTF-8 and the same key is two bytes again.
    feed(&mut t, b"\x1b%G");
    t.clear_reply();
    assert!(t.key(KeyEvent::press(ch('é'), Modifiers::NONE)));
    assert_eq!(t.reply(), "é".as_bytes());
}

/// **The keypad, read out of `[[source: ctlseqs]]` line 2498 rather than
/// transcribed.**
///
/// Twenty-four rows, two columns chosen by DECKPAM/DECKPNM. The application
/// column is *not* uniformly `SS3`: `2` is `CSI B`, `0` is `CSI 2 ~`, `1` is
/// `SS3 F`. A table written from the pattern would be wrong in eight rows, and
/// wrong in a way each application reads as a different keystroke rather than
/// as an error — which is why this reads the specification's own table and
/// compares against it row by row.
///
/// `SP` and `TAB` are the specification's names for two bytes; every other
/// cell is literal.
#[test]
fn the_keypad_matches_the_specifications_table() {
    let src = ctlseqs();
    let rows = table_at(&src, 2498);
    assert_eq!(
        rows.len(),
        24,
        "line 2498 has 24 rows; found {}",
        rows.len()
    );

    let mut checked = 0;
    for (name, numeric, app) in &rows {
        let Some(k) = keypad_of(name) else {
            panic!("no KeypadKey for the table's {name:?}");
        };

        let want_num = spec_bytes(numeric);
        assert_eq!(keypad_press(k, false), want_num, "{name}: numeric column");

        let want_app = spec_bytes(app);
        assert_eq!(
            keypad_press(k, true),
            want_app,
            "{name}: application column"
        );
        checked += 1;
    }
    assert_eq!(checked, 24);
}

/// The table's row labels, which carry the key's function in parentheses.
fn keypad_of(name: &str) -> Option<KeypadKey> {
    let head = name.split_whitespace().next()?;
    Some(match head {
        "Space" => KeypadKey::Space,
        "Tab" => KeypadKey::Tab,
        "Enter" => KeypadKey::Enter,
        "PF1" => KeypadKey::Pf1,
        "PF2" => KeypadKey::Pf2,
        "PF3" => KeypadKey::Pf3,
        "PF4" => KeypadKey::Pf4,
        "*" => KeypadKey::Multiply,
        "+" => KeypadKey::Add,
        "," => KeypadKey::Comma,
        "-" => KeypadKey::Minus,
        "." => KeypadKey::Period,
        "/" => KeypadKey::Divide,
        "0" => KeypadKey::Digit0,
        "1" => KeypadKey::Digit1,
        "2" => KeypadKey::Digit2,
        "3" => KeypadKey::Digit3,
        "4" => KeypadKey::Digit4,
        "5" => KeypadKey::Digit5,
        "6" => KeypadKey::Digit6,
        "7" => KeypadKey::Digit7,
        "8" => KeypadKey::Digit8,
        "9" => KeypadKey::Digit9,
        "=" => KeypadKey::Equal,
        _ => return None,
    })
}

fn keypad_press(k: KeypadKey, app: bool) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if app {
        // DECKPAM. Not DECCKM: the keypad has its own mode, and conflating
        // them is how the cursor keys' introducer leaks into the keypad's.
        t.write_at(BytePos::ZERO, b"\x1b=");
        t.feed(BytePos::ZERO, ReadLen::from_index((b"\x1b=").len()));
        t.clear_reply();
        assert_eq!(t.keypad(), KeypadMode::Application);
    }
    assert!(t.key(KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE)));
    t.reply().to_vec()
}

/// **The VT220 keypad is a different table, and baud does not send it.**
///
/// `[[source: ctlseqs]]` line 2628 gives the VT102/VT220 application keypad,
/// reached through `sunKeyboard`. It disagrees with line 2498 on eleven rows —
/// keypad `2` is `SS3 r` there and `CSI B` here — so they are alternatives and
/// not additions. Same choice, and the same reason, as Home/End at 2485
/// against the VT220 forms at 2672.
///
/// This asserts the disagreement is real rather than assumed, so that if a
/// later edition of ctlseqs reconciles the two tables the reason recorded here
/// stops being true out loud.
#[test]
fn the_vt220_keypad_is_a_different_table_and_is_not_what_baud_sends() {
    let src = ctlseqs();
    let pc = table_at(&src, 2498);
    let vt = table_at(&src, 2628);
    assert_eq!(vt.len(), 24, "line 2628 has 24 rows");

    let mut differ = 0;
    for (name, _, vt_app) in &vt {
        let Some(k) = keypad_of(name) else { continue };
        let Some((_, _, pc_app)) = pc.iter().find(|(n, _, _)| keypad_of(n) == Some(k)) else {
            continue;
        };
        if pc_app == vt_app {
            continue;
        }
        differ += 1;
        // baud sends the PC form. Asserted against the *other* table so the
        // test fails if the wrong one is ever wired in.
        assert_eq!(keypad_press(k, true), spec_bytes(pc_app), "{name}");
        assert_ne!(keypad_press(k, true), spec_bytes(vt_app), "{name}");
    }
    assert_eq!(differ, 11, "the two tables disagree on 11 rows");
}

/// **The VT52 cursor keys, `[[source: ctlseqs]]` line 2703.**
///
/// `ESC A`-`ESC D`, and the specification says in the same breath that they
/// "do not change" — DECCKM is an ANSI mode and a VT52 cannot be in one. So
/// this asserts the mode is *ignored*, which is the part an encoder that
/// reuses the ANSI path would get wrong.
#[test]
fn the_vt52_cursor_keys_match_the_specification_and_ignore_deccm() {
    let src = ctlseqs();
    // `seq_table_at`, not `table_at`: this table's single column is headed
    // "Normal/Application" because the two modes agree, and a two-column
    // reader sees one cell and stops. Zero rows and a passing loop is how a
    // reader reports absence it invented -- hence the count assertion.
    let rows = seq_table_at(&src, 2703);
    assert_eq!(rows.len(), 4, "line 2703 has 4 rows");

    for (name, form) in &rows {
        let code = match name.split_whitespace().nth(1) {
            Some("Up") => KeyCode::Up,
            Some("Down") => KeyCode::Down,
            Some("Right") => KeyCode::Right,
            Some("Left") => KeyCode::Left,
            _ => panic!("unexpected row {name:?}"),
        };
        let want = spec_bytes(form);
        assert_eq!(vt52_press(code, false), want, "{name}");
        assert_eq!(
            vt52_press(code, true),
            want,
            "{name}: DECCKM must not move it"
        );
    }
}

/// **The VT52 keypad, `[[source: ctlseqs]]` line 2713.**
///
/// The application column is `ESC ? <final>` — the one place in this encoder
/// where the introducer is three bytes — and its finals are the VT220 table's,
/// not the PC one's: keypad `2` is `ESC ? r`, where the ANSI PC keypad sends
/// `CSI B`. A VT52 keypad has no cursor duties to borrow.
#[test]
fn the_vt52_keypad_matches_the_specifications_table() {
    let src = ctlseqs();
    let rows = table_at(&src, 2713);
    assert_eq!(rows.len(), 24, "line 2713 has 24 rows");

    for (name, numeric, app) in &rows {
        let Some(k) = keypad_of(name) else {
            panic!("no KeypadKey for {name:?}");
        };
        assert_eq!(
            vt52_keypad_press(k, false),
            spec_bytes(numeric),
            "{name}: numeric column"
        );
        assert_eq!(
            vt52_keypad_press(k, true),
            spec_bytes(app),
            "{name}: application column"
        );
    }
}

/// **A VT52 has no function keys, and baud sends nothing for them.**
///
/// `[[source: ctlseqs]]` line 2698 says so flatly. Neither table gives a form
/// for the function or editing keys, so an encoder that fell through to the
/// ANSI path would send `CSI 15 ~` on a terminal whose parser cannot read a
/// CSI at all — a sequence the application would then see as text.
#[test]
fn a_vt52_sends_nothing_for_keys_it_does_not_have() {
    for code in [
        KeyCode::Function(FnKey::new(1).unwrap()),
        KeyCode::Function(FnKey::new(5).unwrap()),
        KeyCode::Insert,
        KeyCode::Delete,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
    ] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        enter_vt52(&mut t);
        assert!(!t.key(KeyEvent::press(code, Modifiers::NONE)), "{code:?}");
        assert!(t.reply().is_empty(), "{code:?}");
    }
}

/// Leaving VT52 restores the ANSI encoding, so the tables do not leak.
#[test]
fn leaving_vt52_restores_the_ansi_forms() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    enter_vt52(&mut t);
    assert!(t.key(KeyEvent::press(KeyCode::Up, Modifiers::NONE)));
    assert_eq!(t.reply(), b"\x1bA");
    t.clear_reply();

    // `ESC <` is the VT52 sequence that returns to ANSI.
    feed(&mut t, b"\x1b<");
    t.clear_reply();
    assert_eq!(t.terminal_mode(), TerminalMode::Ansi);
    assert!(t.key(KeyEvent::press(KeyCode::Up, Modifiers::NONE)));
    assert_eq!(t.reply(), b"\x1b[A");
}

fn enter_vt52(t: &mut Term) {
    feed(t, b"\x1b[?2l");
    t.clear_reply();
    assert_eq!(t.terminal_mode(), TerminalMode::Vt52);
}

fn vt52_press(code: KeyCode, deccm: bool) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if deccm {
        // Set DECCKM *before* leaving ANSI, so the bit is genuinely on while
        // the VT52 table is in use. Setting it after would be a no-op the
        // test could not tell from the assertion passing.
        feed(&mut t, b"\x1b[?1h");
    }
    enter_vt52(&mut t);
    assert!(t.key(KeyEvent::press(code, Modifiers::NONE)));
    t.reply().to_vec()
}

fn vt52_keypad_press(k: KeypadKey, app: bool) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    enter_vt52(&mut t);
    if app {
        feed(&mut t, b"\x1b=");
        t.clear_reply();
        assert_eq!(t.keypad(), KeypadMode::Application);
    }
    assert!(t.key(KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE)));
    t.reply().to_vec()
}

// ---------------------------------------------------------------------------
// Properties. The tests above are the specification's own tables, which is the
// right oracle for *what* a key sends. These are the invariants that must hold
// for keys no table lists -- every combination of code, modifier and mode --
// and they exist because 23 example tests cannot cover a product of five axes.

use proptest::prelude::*;

fn any_key() -> impl Strategy<Value = KeyCode> {
    prop_oneof![
        (0x20u32..0x10_ffff)
            .prop_map(|v| KeyCode::Char(Codepoint::from_char(char::from_u32(v).unwrap_or('?')))),
        (1u8..=20).prop_map(|n| KeyCode::Function(FnKey::new(n).expect("1..=20"))),
        prop_oneof![
            Just(KeypadKey::Space),
            Just(KeypadKey::Tab),
            Just(KeypadKey::Enter),
            Just(KeypadKey::Pf1),
            Just(KeypadKey::Pf4),
            Just(KeypadKey::Multiply),
            Just(KeypadKey::Minus),
            Just(KeypadKey::Period),
            Just(KeypadKey::Digit0),
            Just(KeypadKey::Digit1),
            Just(KeypadKey::Digit5),
            Just(KeypadKey::Digit9),
            Just(KeypadKey::Equal),
        ]
        .prop_map(KeyCode::Keypad),
        Just(KeyCode::Up),
        Just(KeyCode::Down),
        Just(KeyCode::Right),
        Just(KeyCode::Left),
        Just(KeyCode::Home),
        Just(KeyCode::End),
        Just(KeyCode::Insert),
        Just(KeyCode::Delete),
        Just(KeyCode::PageUp),
        Just(KeyCode::PageDown),
        Just(KeyCode::Enter),
        Just(KeyCode::Tab),
        Just(KeyCode::Backspace),
        Just(KeyCode::Escape),
    ]
}

/// Every mode combination a key can be encoded under, as the bytes that select
/// them. Modes are not independent of the encoder: DECCKM, DECKPAM, LNM, VT52
/// and the two `ESC`-prefix modes each change what some key sends.
fn any_modes() -> impl Strategy<Value = Vec<&'static [u8]>> {
    proptest::collection::vec(
        prop_oneof![
            Just(&b"\x1b[?1h"[..]), // DECCKM
            Just(&b"\x1b="[..]),    // DECKPAM
            Just(&b"\x1b[20h"[..]), // LNM
            Just(&b"\x1b[?1036l"[..]),
            Just(&b"\x1b[?1039l"[..]),
            Just(&b"\x1b[?1034h"[..]),
            Just(&b"\x1b%@"[..]),   // ISO 8859-1
            Just(&b"\x1b[?2l"[..]), // VT52
        ],
        0..5,
    )
}

fn encode(code: KeyCode, mods: Modifiers, setup: &[&[u8]]) -> (bool, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for m in setup {
        feed(&mut t, m);
    }
    t.clear_reply();
    let sent = t.key(KeyEvent::press(code, mods));
    (sent, t.reply().to_vec())
}

proptest! {
    /// **Nothing sent means nothing written, and something sent is never
    /// empty.** The return value is the only signal an embedder has, so a
    /// `false` that still wrote bytes would corrupt the stream and a `true`
    /// with no bytes would look like a dropped keystroke.
    #[test]
    fn the_return_value_agrees_with_the_bytes(
        code in any_key(),
        bits in 0u8..16,
        setup in any_modes(),
    ) {
        let mods = Modifiers::from_wire(bits + 1).expect("1..=16");
        let (sent, out) = encode(code, mods, &setup);
        prop_assert_eq!(sent, !out.is_empty());
    }

    /// **A key press never emits a bare `ESC` followed by nothing.**
    ///
    /// `ESC` alone is a keystroke; `ESC` as the first byte of a sequence that
    /// never arrives is a terminal the application cannot resynchronise. The
    /// only key that may send a lone `ESC` is Escape itself.
    #[test]
    fn only_escape_sends_a_lone_escape(
        code in any_key(),
        bits in 0u8..16,
        setup in any_modes(),
    ) {
        let mods = Modifiers::from_wire(bits + 1).expect("1..=16");
        let (_, out) = encode(code, mods, &setup);
        if out == [0x1b] {
            prop_assert!(matches!(code, KeyCode::Escape));
        }
    }

    /// **A release is never encoded on this layer, whatever the mode.**
    ///
    /// `[[source: kitty-keyboard]]`'s event types are what report releases.
    /// An application that has not asked for them must never see one -- it
    /// would read the release as a second press.
    #[test]
    fn a_release_is_never_sent(
        code in any_key(),
        bits in 0u8..16,
        setup in any_modes(),
    ) {
        let mods = Modifiers::from_wire(bits + 1).expect("1..=16");
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for m in &setup {
            feed(&mut t, m);
        }
        t.clear_reply();
        let kind = KeyEventKind::Release;
        let sent = t.key(KeyEvent { code, mods, kind, text: None, shifted: None, base_layout: None });
        prop_assert!(!sent);
        prop_assert!(t.reply().is_empty());
    }

    /// **A repeat sends exactly what a press of the same key sends**, under
    /// every mode this suite reaches: `[[source: kitty-keyboard]]` treats a
    /// repeat as a press wherever event types are not reported.
    #[test]
    fn a_repeat_sends_what_a_press_sends(
        code in any_key(),
        bits in 0u8..16,
        setup in any_modes(),
    ) {
        let mods = Modifiers::from_wire(bits + 1).expect("1..=16");
        let pressed = encode(code, mods, &setup);
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for m in &setup {
            feed(&mut t, m);
        }
        t.clear_reply();
        let kind = KeyEventKind::Repeat;
        let sent = t.key(KeyEvent { kind, ..KeyEvent::press(code, mods) });
        prop_assert_eq!((sent, t.reply().to_vec()), pressed);
    }

    /// **A modified key never uses `SS3`.**
    ///
    /// `SS3` takes no parameters, so there is nowhere to put the modifier;
    /// `[[source: ctlseqs]]` line 2564 states it for F1-F4 and the same holds
    /// for the cursor keys. An encoder that kept `SS3` emits something no
    /// application parses, and this is the rule the keypad broke by reading
    /// DECCKM instead of its own table.
    #[test]
    fn a_modifier_never_leaves_ss3_in_front(
        code in any_key(),
        bits in 1u8..16,
        setup in any_modes(),
    ) {
        // `bits + 1` is 2..=16: at least one modifier held.
        let mods = Modifiers::from_wire(bits + 1).expect("2..=16");
        let (_, out) = encode(code, mods, &setup);
        // **The keypad is exempt, and that is a scope decision rather than a
        // hole.** `[[source: ctlseqs]]` gives the `CSI 1 ; mod` shape for the
        // cursor and function keys and gives *nothing* for `SS3 j` and its
        // neighbours, so baud sends the unmodified form rather than inventing
        // one. This property found that by failing on `Shift` + keypad `=`,
        // and the answer was to narrow the property, not the encoder.
        let exempt = matches!(code, KeyCode::Keypad(_));
        let is_ss3 = out.starts_with(b"\x1bO");
        prop_assert!(exempt || !is_ss3, "modified key emitted SS3: {:?}", out);
    }

    /// The reply buffer is bounded and a key must never be able to overflow it.
    #[test]
    fn a_single_key_is_short(
        code in any_key(),
        bits in 0u8..16,
        setup in any_modes(),
    ) {
        let mods = Modifiers::from_wire(bits + 1).expect("1..=16");
        let (_, out) = encode(code, mods, &setup);
        // The longest this generator reaches is `CSI <param> ; <mod> ~` with a
        // two-digit parameter (F5-F20) and a two-digit modifier (10-16), such
        // as `CSI 34 ; 16 ~`: 8 bytes. A tilde key carries its modifier as a
        // parameter and never takes Alt's `ESC` prefix; the prefix goes only on
        // a character key, which is at most `ESC` and four bytes of UTF-8. The
        // generator sets neither `modifyOtherKeys` nor a kitty flag, and
        // `Term::key` sends 7-bit controls; those layers reach 14 and 40
        // bytes, and `baud-domain/tests/key_bytes.rs` measures both.
        prop_assert!(out.len() <= 8, "{} bytes: {:?}", out.len(), out);
    }
}

/// **None, and the gate stays so a new one has to say so.**
///
/// The two that were here were the encoding gap wearing an input-side face:
/// `Term::key` reads `self.cold.encoding` and nothing wrote it, so the core
/// was always UTF-8 and the Latin-1 rows could not pass. They closed when
/// `ESC % @` was routed, without touching the encoder -- which is the point of
/// `baud_domain::encode_key` being a pure function of the state `Term::key`
/// hands it.
#[test]
fn the_known_key_gaps() {
    support::ignores_are(".", &["key_encode.rs"], &[]);
}
