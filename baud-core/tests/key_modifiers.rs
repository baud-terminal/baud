#![deny(dead_code)]
//! xterm's key-modifier resources, every one: XTMODKEYS `CSI > Pp ; Pv m`
//! for `Pp` 0, 1, 2, 3, 4, 6 and 7, XTFMTKEYS `CSI > Pp ; Pv f` for the
//! same, the disable form `CSI > Ps n`, and the queries that read them back.
//!
//! `[[source: ctlseqs]]` names the resources and the controls; the values
//! are `[[source: xterm-man]]`'s (`modifyCursorKeys` -1 to 4 and its
//! siblings, `modifyOtherKeys` 0 to 3, `modifyKeyboard`'s four bits), and
//! what each value sends is `[[source: xterm-input-c]]`'s, the reference
//! the text describes. The extended form's number is a key's keysym
//! through `keysym2ucs` (`[[source: xterm-keysym2ucs-c]]`): its table where
//! it has a row, else the keysym less 0x1d00, which ctlseqs states as
//! "XTerm maps them to the Unicode BMP private use area beginning at
//! U+E001".

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// Feed `setup`, clear the reply, press `code` with `mods`, and return what
/// the key sent: empty when it sends nothing.
fn sent(setup: &[u8], code: KeyCode, mods: Modifiers) -> Vec<u8> {
    support::with_corpus(|t| {
        support::feed(t, setup);
        t.clear_reply();
        t.key(KeyEvent::press(code, mods));
        t.reply().to_vec()
    })
}

/// The reply to `query` after `setup`.
fn answer(setup: &[u8], query: &[u8]) -> Vec<u8> {
    support::with_corpus(|t| {
        support::feed(t, setup);
        t.clear_reply();
        support::feed(t, query);
        t.reply().to_vec()
    })
}

fn show(b: &[u8]) -> String {
    String::from_utf8_lossy(b).replace('\x1b', "ESC ")
}

fn ch(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

fn f(n: u8) -> KeyCode {
    KeyCode::Function(FnKey::new(n).expect("a function key"))
}

const CTRL: Modifiers = Modifiers::CTRL;

/// **modifyOtherKeys 3 sends unmodified keys too**: "If modifyOtherKeys is
/// set to 3, unmodified keys also are sent as escape sequences. For
/// example, space sends CSI 2 7 ; 1 ; 3 2 ~" (`[[source: ctlseqs]]`), read
/// from the pinned text. xterm's `ModifyOtherKeys` takes every ordinary key
/// at 3, Return, Tab, Backspace and Escape among them.
#[test]
fn modify_other_keys_3_sends_every_ordinary_key() {
    let doc = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs");
    let line = doc
        .lines()
        .find(|l| l.contains("For example, space sends CSI"))
        .expect("the level-3 example");
    let spaced = line.split("sends CSI").nth(1).expect("its bytes");
    let mut want = b"\x1b[".to_vec();
    want.extend(spaced.bytes().filter(|b| !b.is_ascii_whitespace()));
    assert_eq!(show(&want), show(b"\x1b[27;1;32~"), "the scrape");
    let three = b"\x1b[>4;3m";
    assert_eq!(show(&sent(three, ch(' '), Modifiers::NONE)), show(&want));
    for (code, mods, bytes) in [
        (ch('a'), Modifiers::NONE, &b"\x1b[27;1;97~"[..]),
        (ch('a'), CTRL, b"\x1b[27;5;97~"),
        (KeyCode::Enter, Modifiers::NONE, b"\x1b[27;1;13~"),
        (KeyCode::Tab, Modifiers::NONE, b"\x1b[27;1;9~"),
        (KeyCode::Tab, Modifiers::SHIFT, b"\x1b[27;2;9~"),
        (KeyCode::Backspace, Modifiers::NONE, b"\x1b[27;1;127~"),
        (KeyCode::Escape, Modifiers::NONE, b"\x1b[27;1;27~"),
        // A special key is not an ordinary one: Up keeps its form.
        (KeyCode::Up, Modifiers::NONE, b"\x1b[A"),
    ] {
        assert_eq!(
            show(&sent(three, code, mods)),
            show(bytes),
            "{code:?} {mods:?}"
        );
    }
    let csi_u = b"\x1b[>4;3m\x1b[>4;1f";
    assert_eq!(
        show(&sent(csi_u, ch(' '), Modifiers::NONE)),
        show(b"\x1b[32;1u")
    );
    // Below 3 an unmodified key is its character.
    assert_eq!(sent(b"\x1b[>4;2m", ch(' '), Modifiers::NONE), b" ");
}

/// **modifyCursorKeys, each value**, Control-Up in normal and application
/// cursor mode, from `modifyCursorKey` (`[[source: xterm-input-c]]`): -1
/// drops the modifier, 0 keeps `SS3` with the modifier first, 1 forces
/// `CSI`, 2 (the first value) puts it second, 3 marks the sequence with
/// `>`, and 4 is the extended form, Up's keysym 0xff52 less 0x1d00, sent
/// unmodified too. PageUp, an editing key, takes no `modifyCursorKey`
/// below 4.
#[test]
fn modify_cursor_keys_each_value() {
    for (set, normal, app) in [
        (&b"\x1b[>1n"[..], &b"\x1b[A"[..], &b"\x1bOA"[..]),
        (b"\x1b[>1;0m", b"\x1b[5A", b"\x1bO5A"),
        (b"\x1b[>1;1m", b"\x1b[5A", b"\x1b[5A"),
        (b"\x1b[>1;2m", b"\x1b[1;5A", b"\x1b[1;5A"),
        (b"", b"\x1b[1;5A", b"\x1b[1;5A"),
        (b"\x1b[>1;3m", b"\x1b[>1;5A", b"\x1b[>1;5A"),
        (b"\x1b[>1;4m", b"\x1b[27;5;57938~", b"\x1b[27;5;57938~"),
    ] {
        let both = [set, b"\x1b[?1h"].concat();
        assert_eq!(
            show(&sent(set, KeyCode::Up, CTRL)),
            show(normal),
            "{}",
            show(set)
        );
        assert_eq!(
            show(&sent(&both, KeyCode::Up, CTRL)),
            show(app),
            "{} DECCKM",
            show(set)
        );
        let page = sent(set, KeyCode::PageUp, CTRL);
        if set != b"\x1b[>1;4m" {
            assert_eq!(show(&page), show(b"\x1b[5;5~"), "{} PageUp", show(set));
        }
    }
    let ext = b"\x1b[>1;4m";
    assert_eq!(
        show(&sent(ext, KeyCode::Up, Modifiers::NONE)),
        show(b"\x1b[27;1;57938~")
    );
    assert_eq!(
        show(&sent(ext, KeyCode::PageUp, CTRL)),
        show(b"\x1b[27;5;57941~")
    );
    assert_eq!(
        show(&sent(ext, KeyCode::Delete, Modifiers::NONE)),
        show(b"\x1b[27;1;127~")
    );
    let fmt = b"\x1b[>1;4m\x1b[>1;1f";
    assert_eq!(show(&sent(fmt, KeyCode::Up, CTRL)), show(b"\x1b[57938;5u"));
    // Unmodified below 4, nothing changes.
    assert_eq!(sent(b"\x1b[>1;3m", KeyCode::Up, Modifiers::NONE), b"\x1b[A");
}

/// **modifyFunctionKeys, each value.** At -1 "xterm uses the modifier keys
/// to make an extended sequence of function keys rather than adding a
/// parameter" (`[[source: ctlseqs]]`): Shift adds `ctrlFKeys`, 10, and
/// Control twice it, and past F20 xterm numbers keys 42 upward
/// (`decfuncvalue`); any other modifier is dropped. F1-F4 take
/// `modifyCursorKey`'s forms, F5 up keep the modifier second below 3.
#[test]
fn modify_function_keys_each_value() {
    let off = b"\x1b[>2n";
    for (code, mods, bytes) in [
        (f(1), Modifiers::SHIFT, &b"\x1b[23~"[..]),
        (f(1), CTRL, b"\x1b[42~"),
        (f(1), CTRL.with(Modifiers::SHIFT), b"\x1b[52~"),
        (f(1), Modifiers::ALT, b"\x1bOP"),
        (f(5), CTRL, b"\x1b[46~"),
        (f(2), Modifiers::NONE, b"\x1bOQ"),
    ] {
        assert_eq!(
            show(&sent(off, code, mods)),
            show(bytes),
            "-1 {code:?} {mods:?}"
        );
    }
    assert_eq!(
        sent(b"\x1b[>n", f(1), Modifiers::SHIFT),
        b"\x1b[23~",
        "CSI > n"
    );
    for (set, f1, f5) in [
        (&b"\x1b[>2;0m"[..], &b"\x1bO5P"[..], &b"\x1b[15;5~"[..]),
        (b"\x1b[>2;1m", b"\x1b[5P", b"\x1b[15;5~"),
        (b"", b"\x1b[1;5P", b"\x1b[15;5~"),
        (b"\x1b[>2;3m", b"\x1b[>1;5P", b"\x1b[>15;5~"),
        (b"\x1b[>2;4m", b"\x1b[27;5;58046~", b"\x1b[27;5;58050~"),
    ] {
        assert_eq!(show(&sent(set, f(1), CTRL)), show(f1), "{} F1", show(set));
        assert_eq!(show(&sent(set, f(5), CTRL)), show(f5), "{} F5", show(set));
    }
    // The keypad's PF1 is not a function key: no remap.
    assert_eq!(
        sent(off, KeyCode::Keypad(KeypadKey::Pf1), Modifiers::SHIFT),
        b"\x1b[1;2P"
    );
}

/// **modifyKeypadKeys, modifyModifierKeys and modifySpecialKeys act at 4**,
/// "Like modifyCursorKeys 4" (`[[source: xterm-man]]`); xterm reads each as
/// `>= mfkExtended` and nothing else, so below 4 a key is unchanged. Each
/// takes its own format resource. A media key's keysym is past what a
/// parameter names, so it sends nothing, as it did.
#[test]
fn the_keypad_modifier_and_special_resources_act_at_4() {
    let kp1 = KeyCode::Keypad(KeypadKey::Digit1);
    assert_eq!(sent(b"\x1b[>3;3m", kp1, Modifiers::NONE), b"1");
    assert_eq!(
        show(&sent(b"\x1b[>3;4m", kp1, Modifiers::NONE)),
        show(b"\x1b[27;1;58033~")
    );
    assert_eq!(
        show(&sent(b"\x1b[>3;4m\x1b[>3;1f", kp1, CTRL)),
        show(b"\x1b[58033;5u")
    );
    let shift_l = KeyCode::Pua(PuaKey::LeftShift);
    assert!(sent(b"\x1b[>6;3m", shift_l, Modifiers::NONE).is_empty());
    assert_eq!(
        show(&sent(b"\x1b[>6;4m", shift_l, Modifiers::NONE)),
        show(b"\x1b[27;1;58081~")
    );
    let special = b"\x1b[>7;4m";
    for (code, mods, bytes) in [
        (KeyCode::Escape, Modifiers::NONE, &b"\x1b[27;1;27~"[..]),
        (KeyCode::Backspace, Modifiers::NONE, b"\x1b[27;1;8~"),
        (KeyCode::Enter, CTRL, b"\x1b[27;5;13~"),
        (KeyCode::Tab, Modifiers::NONE, b"\x1b[27;1;9~"),
        (KeyCode::Tab, Modifiers::SHIFT, b"\x1b[27;2;57632~"),
        (
            KeyCode::Pua(PuaKey::Pause),
            Modifiers::NONE,
            b"\x1b[27;1;19~",
        ),
    ] {
        assert_eq!(show(&sent(special, code, mods)), show(bytes), "{code:?}");
    }
    assert!(sent(special, KeyCode::Pua(PuaKey::MediaPlay), Modifiers::NONE).is_empty());
    // `?1037` set makes Delete a special key, reset an editing one.
    assert_eq!(
        show(&sent(
            b"\x1b[>7;4m\x1b[?1037h",
            KeyCode::Delete,
            Modifiers::NONE
        )),
        show(b"\x1b[27;1;127~")
    );
    assert_eq!(
        sent(b"\x1b[>7;4m", KeyCode::Delete, Modifiers::NONE),
        b"\x1b[3~"
    );
    // Below 4, Escape is Escape.
    assert_eq!(
        sent(b"\x1b[>7;3m", KeyCode::Escape, Modifiers::NONE),
        b"\x1b"
    );
    // A keypad arrow is a keypad key to xterm's `TypeOfKeysym`, so
    // modifyKeypadKeys decides its extended form; only then is it moved onto
    // the arrow, and modifyCursorKeys 4 reaches it as `modifyCursorKey`
    // does, marked `>` with the modifier second.
    let kp_up = KeyCode::Keypad(KeypadKey::Up);
    assert_eq!(
        show(&sent(b"\x1b[>3;4m", kp_up, CTRL)),
        show(b"\x1b[27;5;58007~")
    );
    assert_eq!(show(&sent(b"\x1b[>1;4m", kp_up, CTRL)), show(b"\x1b[>1;5A"));
    assert_eq!(sent(b"\x1b[>1;4m", kp_up, Modifiers::NONE), b"\x1b[A");
}

/// **The ignore mask factors a modifier out**: `CSI > 4 ; Pv : mask m`, as
/// xterm parses it (Jev's lean, CODE_FORM, 0.69/0.88 then 0.80/0.89), the
/// mask in the parameter's bits ("a mask of modifier bits to factor out of
/// the parameter encoding", `[[source: ctlseqs]]`). At levels 1 and 2 a key
/// whose only modifier is in the mask is not sent as a sequence, xterm's
/// `allowedCharModifiers`; with another held it is. At 3, where an
/// unmodified key is a sequence too, the masked key is sent with the
/// modifier factored out, as xterm's min_mod 0 there gives it.
#[test]
fn the_ignore_mask_factors_a_modifier_out() {
    assert_eq!(
        show(&sent(b"\x1b[>4;3:1m", ch('a'), Modifiers::SHIFT)),
        show(b"\x1b[27;1;97~"),
        "level 3: the modifier factored out"
    );
    assert_eq!(
        show(&sent(b"\x1b[>4;3:1m", ch('a'), CTRL.with(Modifiers::SHIFT))),
        show(b"\x1b[27;6;97~")
    );
    // Shift-Tab at 3 is a Tab with Shift factored out. xterm's Shift-Tab is
    // the keysym ISO_Left_Tab, not an ordinary key, so its mask never
    // applies there and it sends `CSI 27 ; 2 ; 9 ~`: an open departure.
    assert_eq!(
        show(&sent(b"\x1b[>4;3:1m", KeyCode::Tab, Modifiers::SHIFT)),
        show(b"\x1b[27;1;9~"),
        "level 3: Shift-Tab sends an unmodified Tab's bytes"
    );
    let masked = b"\x1b[>4;2:1m";
    assert_eq!(sent(masked, KeyCode::Tab, Modifiers::SHIFT), b"\x1b[Z");
    assert_eq!(
        show(&sent(masked, ch('a'), CTRL.with(Modifiers::SHIFT))),
        show(b"\x1b[27;6;97~")
    );
    assert_eq!(show(&sent(masked, ch('a'), CTRL)), show(b"\x1b[27;5;97~"));
    // A level set without a mask clears it.
    assert_eq!(
        show(&sent(
            b"\x1b[>4;2:1m\x1b[>4;2m",
            KeyCode::Tab,
            Modifiers::SHIFT
        )),
        show(b"\x1b[27;2;9~")
    );
}

/// **Each control resets, sets or disables what the text says.** An
/// omitted `Pv` resets one resource, no parameter at all every one of the
/// seven (ctlseqs' "all resources", Jev's lean, ALL_SEVEN, 0.76/0.76 then
/// 0.68/0.76), `CSI > Ps n` disables `Ps`, and with no parameter
/// modifyFunctionKeys.
/// DECSTR and RIS put every one back, as xterm's `ReallyReset` does.
#[test]
fn the_controls_reset_set_and_disable() {
    let probe = b"\x1b[?0;1;2;3;4;6;7m\x1b[>0;1;2;3;4;6;7g";
    let first = answer(b"", probe);
    assert_eq!(
        show(&first),
        show(
            b"\x1b[>0;0m\x1b[>1;2m\x1b[>2;2m\x1b[>3;0m\x1b[>4;0m\x1b[>6;0m\x1b[>7;0m\
              \x1b[>0;0f\x1b[>1;0f\x1b[>2;0f\x1b[>3;0f\x1b[>4;0f\x1b[>6;0f\x1b[>7;0f"
        )
    );
    let every = b"\x1b[>0;15m\x1b[>1;3m\x1b[>2n\x1b[>3;4m\x1b[>4;3m\x1b[>6;1m\x1b[>7;2m\
                  \x1b[>0;1f\x1b[>1;1f\x1b[>2;1f\x1b[>3;1f\x1b[>4;1f\x1b[>6;1f\x1b[>7;1f";
    let moved = answer(every, probe);
    assert_eq!(
        show(&moved),
        show(
            b"\x1b[>0;15m\x1b[>1;3m\x1b[>2;65535m\x1b[>3;4m\x1b[>4;3m\x1b[>6;1m\x1b[>7;2m\
              \x1b[>0;1f\x1b[>1;1f\x1b[>2;1f\x1b[>3;1f\x1b[>4;1f\x1b[>6;1f\x1b[>7;1f"
        ),
        "the control: every resource moved"
    );
    for reset in [&b"\x1b[>m\x1b[>f"[..], b"\x1b[!p", b"\x1bc"] {
        assert_eq!(
            show(&answer(&[every, reset].concat(), probe)),
            show(&first),
            "{}",
            show(reset)
        );
    }
    assert_eq!(
        show(&answer(
            b"\x1b[>1;3m\x1b[>1m\x1b[>1;1f\x1b[>1f",
            b"\x1b[?1m\x1b[>1g"
        )),
        show(b"\x1b[>1;2m\x1b[>1;0f"),
        "an omitted Pv resets the one"
    );
    assert_eq!(
        show(&answer(b"\x1b[>n\x1b[>1n", b"\x1b[?2;1m")),
        show(b"\x1b[>2;65535m\x1b[>1;65535m"),
        "CSI > n disables modifyFunctionKeys"
    );
    // Open departure: xterm's answer for a disabled resource, -1 written
    // unsigned, cannot be fed back, because baud refuses 65535 as a value.
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[>2n");
        t.clear_reply();
        support::feed(t, b"\x1b[?2m");
        let back = t.reply().to_vec();
        assert_eq!(show(&back), show(b"\x1b[>2;65535m"));
        support::feed(t, b"\x1b[!p");
        let before = t.unsupported;
        support::feed(t, &back);
        assert_eq!(t.unsupported, before + 1, "the answer is refused");
        t.clear_reply();
        support::feed(t, b"\x1b[?2m");
        assert_eq!(show(t.reply()), show(b"\x1b[>2;2m"), "and nothing moved");
    });
}

/// **A value no resource documents is refused and counted**, the resource
/// unchanged (Jev's lean, REFUSE_COUNT, 0.82/0.90 then 0.88/0.86), as
/// `CSI > 4 ; 2 f` already was: modifyOtherKeys past 3, the others past 4,
/// modifyKeyboard past its four bits, a format past 1, the reserved
/// resource 5, a `Pp` that names none, and the mask on any resource but 4.
#[test]
fn an_undocumented_value_is_refused_and_counted() {
    support::with_corpus(|t| {
        let before = t.unsupported;
        let refused: [&[u8]; 8] = [
            b"\x1b[>4;4m",
            b"\x1b[>1;5m",
            b"\x1b[>0;16m",
            b"\x1b[>2;2f",
            b"\x1b[>5;1m",
            b"\x1b[>8;1m",
            b"\x1b[>9n",
            b"\x1b[>1;2:1m",
        ];
        for b in refused {
            support::feed(t, b);
        }
        assert_eq!(t.unsupported - before, refused.len(), "each is counted");
        t.clear_reply();
        support::feed(t, b"\x1b[?4;1;2;0m\x1b[>2g");
        assert_eq!(
            show(t.reply()),
            show(b"\x1b[>4;0m\x1b[>1;2m\x1b[>2;2m\x1b[>0;0m\x1b[>2;0f"),
            "and nothing moved"
        );
    });
}

/// **Open departure: ctlseqs' own example of the mask is refused.**
/// "For example CSI > 4 : 1 m factors out the shift modifier"
/// (`[[source: ctlseqs]]`) puts the mask on `Pp`; xterm's parser reads it
/// only as `Pv`'s sub-parameter, `CSI > 4 ; Pv : mask m`, and baud reads it
/// as xterm does (Jev's lean, CODE_FORM, 0.69/0.88 then 0.80/0.89, below
/// the bar). The text's form is counted and changes nothing.
#[test]
fn the_texts_mask_example_is_refused_and_counted() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[>4;2m");
        let before = t.unsupported;
        support::feed(t, b"\x1b[>4:1m");
        assert_eq!(t.unsupported, before + 1, "counted");
        t.clear_reply();
        support::feed(t, b"\x1b[?4m");
        assert_eq!(show(t.reply()), show(b"\x1b[>4;2m"), "and nothing moved");
    });
    assert_eq!(
        show(&sent(
            b"\x1b[>4;2m\x1b[>4:1m",
            KeyCode::Tab,
            Modifiers::SHIFT
        )),
        show(b"\x1b[27;2;9~"),
        "Shift is not factored out"
    );
}

/// **XTQMODKEYS answers every resource as xterm's `report_mod_fkeys`**:
/// `CSI > Pp ; Pv m`, and for a `Pp` that names none the value xterm writes
/// for its `DEFAULT`, -1 as an unsigned short, 65535 (Jev's lean,
/// XTERM_65535), and so for a disabled one, -1 as xterm writes it, the
/// first ask's lean in both orders. DECRQSS answers a
/// known one `DCS 1 $ r > Pp ; Pv m ST`, -1 written as -1 and the mask as
/// `:mask`, as xterm's `do_dcs` does (XTERM_MINUS_ONE), and an unknown one
/// `DCS 0 $ r ST`.
#[test]
fn every_resource_answers_as_xterm_reports_it() {
    support::with_corpus(|t| {
        let before = t.unsupported_param;
        support::feed(t, b"\x1b[?5m\x1b[>9g");
        assert_eq!(show(t.reply()), show(b"\x1b[>5;65535m\x1b[>9;65535f"));
        assert_eq!(t.unsupported_param, before + 2, "an unknown one counted");
    });
    for (setup, query, reply) in [
        (
            &b""[..],
            &b"\x1bP$q>1m\x1b\\"[..],
            &b"\x1bP1$r>1;2m\x1b\\"[..],
        ),
        (b"\x1b[>2n", b"\x1b[?2m", b"\x1b[>2;65535m"),
        (b"\x1b[>2n", b"\x1bP$q>2m\x1b\\", b"\x1bP1$r>2;-1m\x1b\\"),
        (
            b"\x1b[>4;2:5m",
            b"\x1bP$q>4m\x1b\\",
            b"\x1bP1$r>4;2:5m\x1b\\",
        ),
        (b"\x1b[>7;1f", b"\x1bP$q>7f\x1b\\", b"\x1bP1$r>7;1f\x1b\\"),
        (b"", b"\x1bP$q>5m\x1b\\", b"\x1bP0$r\x1b\\"),
        (b"", b"\x1bP$q>0f\x1b\\", b"\x1bP1$r>0;0f\x1b\\"),
    ] {
        assert_eq!(show(&answer(setup, query)), show(reply), "{}", show(query));
    }
}
