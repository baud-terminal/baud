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

use baud_core::*;

fn ctlseqs() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs")
}

/// The `Key | Normal | Application` table at the given line, as
/// `(name, normal, application)`.
fn table_at(src: &str, line: usize) -> Vec<(String, String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        if t.starts_with('-') {
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    if app {
        // DECCKM set: cursor keys go application.
        let b = b"\x1b[?1h";
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
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

/// Releases and repeats are not sent on this layer.
///
/// Only `[[source: kitty-keyboard]]` reports them, and only once an
/// application has asked. A terminal that volunteered them would be sending
/// bytes nothing requested.
#[test]
fn only_presses_are_encoded() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let e = KeyEvent {
            code: KeyCode::Up,
            mods: Modifiers::NONE,
            kind,
        };
        assert!(!t.key(e), "{kind:?} must not be encoded");
        assert!(t.reply().is_empty(), "and must write nothing");
    }
}

/// The `Key | Escape Sequence` table at the given line.
fn seq_table_at(src: &str, line: usize) -> Vec<(String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        if t.starts_with('-') {
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(b"\x1b[20h"));
    t.feed(base, f.len());
    t.commit_read(f.len());
    t.clear_reply();

    assert!(t.key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE)));
    assert_eq!(t.reply(), &[0x0d, 0x0a], "LNM set: CR LF");
}

fn feed(t: &mut Term, bytes: &[u8]) {
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
