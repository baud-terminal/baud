//! KAM (`CSI 2 h`) and SRM (`CSI 12 l`): the keyboard locked, and the keys
//! a user types drawn by the terminal itself. And the DEC private modes
//! that choose what a key sends: DECNKM `?66`, DECBKM `?67`, `?1035` and
//! `?1037`, below.
//!
//! `[[source: vt510-kam]]`: "If KAM is set, then the keyboard cannot send
//! characters to the host ... The terminal ignores all keystrokes that send
//! characters to the host." `[[source: vt510-srm]]`: "When the SRM function is
//! reset, the terminal sends keyboard characters to the host and to the
//! screen", "Default: No local echo". `[[source: ecma48]]` 7.2.11 and 7.2.15
//! give the same two modes as ENABLED/DISABLED and MONITOR/SIMULTANEOUS.
//! Decided by Jev 2026-10-19: only the keys are locked and only the keys are
//! echoed (KEYS_ONLY, 1.00/1.00 both), through a parser state of the echo's
//! own (OWN_STATE, 1.00/1.00), the lock exposed rather than drawn
//! (EXPOSE_STATE, 1.00/1.00) and built now (BUILD_NOW, 0.98/0.96); SRM set at
//! power-on, Jev's lean (SET_NO_ECHO, 0.80/0.72).

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

fn press(t: &mut Term<'_>, code: KeyCode) -> bool {
    t.key(KeyEvent::press(code, Modifiers::NONE))
}

fn ch(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

fn cell(t: &Term<'_>, y: usize, x: usize) -> Option<char> {
    let c = t.row(Row::from_index(y)).expect("a row")[x];
    c.content
        .as_codepoint()
        .and_then(|cp| char::from_u32(cp.value()))
}

fn cursor(t: &Term<'_>) -> (usize, usize) {
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

// ── KAM ─────────────────────────────────────────────────────────────────────

#[test]
fn kam_locks_the_keyboard_and_a_key_sends_nothing() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[2h");
        t.clear_reply();
        assert!(t.keyboard_locked());
        assert!(!press(t, ch('a')), "nothing sent");
        assert!(!press(t, KeyCode::Up));
        assert_eq!(t.reply(), b"");
        support::feed(t, b"\x1b[2l");
        assert!(!t.keyboard_locked());
        assert!(press(t, ch('a')), "unlocked");
        assert_eq!(t.reply(), b"a");
    });
}

/// "All keystrokes that send characters": the keyboard, and not a focus
/// report or a mouse report, which the terminal generates on its own.
#[test]
fn kam_leaves_the_focus_report_alone() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1004h\x1b[2h");
        t.clear_reply();
        assert!(t.focus(Focus::Gained));
        assert_eq!(t.reply(), b"\x1b[I");
    });
}

#[test]
fn kam_is_reported_and_unlocked_by_decstr_and_ris() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[2$p\x1b[2h\x1b[2$p");
        assert_eq!(t.reply(), b"\x1b[2;2$y\x1b[2;1$y");
        assert_eq!(t.unsupported_param, 0, "neither counted");
        support::feed(t, b"\x1b[!p");
        assert!(!t.keyboard_locked(), "DECSTR: Unlocked (VT510 Table 5-9)");
        support::feed(t, b"\x1b[2h\x1bc");
        assert!(!t.keyboard_locked(), "RIS");
    });
}

// ── SRM ─────────────────────────────────────────────────────────────────────

#[test]
fn srm_is_set_at_power_on_and_a_key_is_not_drawn() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12$p");
        assert_eq!(t.reply(), b"\x1b[12;1$y");
        assert!(!t.local_echo());
        t.clear_reply();
        assert!(press(t, ch('a')));
        assert_eq!(t.reply(), b"a", "sent");
        assert_eq!(cell(t, 0, 0), None, "and not drawn");
    });
}

#[test]
fn srm_reset_draws_a_typed_key_as_well_as_sending_it() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[3;5H");
        t.clear_reply();
        assert!(t.local_echo());
        assert!(press(t, ch('a')));
        assert!(press(t, ch('b')));
        assert_eq!(t.reply(), b"ab", "sent");
        assert_eq!(
            (cell(t, 2, 4), cell(t, 2, 5)),
            (Some('a'), Some('b')),
            "drawn"
        );
        assert_eq!(cursor(t), (2, 6));
    });
}

/// The echo is what the bytes do: an arrow key's `CSI A` moves the cursor,
/// Return's `CR` returns it.
#[test]
fn the_echo_of_a_sequence_is_performed() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[6;10H");
        press(t, KeyCode::Up);
        assert_eq!(cursor(t), (4, 9));
        press(t, KeyCode::Enter);
        assert_eq!(cursor(t), (4, 0));
    });
}

/// **The echo has a parser of its own**: a key typed between the halves of
/// a host sequence split across reads lands on its own, and the host's
/// sequence completes as if nothing came between.
#[test]
fn the_echo_does_not_land_inside_a_host_sequence() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[1;1H\x1b[3");
        press(t, ch('q'));
        support::feed(t, b"C");
        assert_eq!(cell(t, 0, 0), Some('q'), "the key drawn");
        assert_eq!(cursor(t), (0, 4), "and the host's CUF 3 performed");
        assert_eq!(t.unsupported, 0);
    });
}

/// And keeps its state from key to key: Escape, `[`, `A` typed one at a
/// time are `CSI A`.
#[test]
fn the_echo_keeps_its_state_across_keys() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[6;10H");
        press(t, KeyCode::Escape);
        press(t, ch('['));
        press(t, ch('A'));
        assert_eq!(cursor(t), (4, 9));
        assert_eq!(cell(t, 5, 9), None, "nothing printed");
    });
}

/// The echo's bytes are not the host's: no byte ingested is counted, and
/// a host read after it parses as it would have.
#[test]
fn the_echo_counts_no_byte_in() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l");
        let (before, _) = t.amplification();
        press(t, ch('x'));
        assert_eq!(t.amplification().0, before);
    });
}

#[test]
fn a_locked_keyboard_echoes_nothing() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[2h");
        press(t, ch('a'));
        assert_eq!(cell(t, 0, 0), None);
    });
}

/// The wheel's cursor keys under `?1007` are the mouse's, not keystrokes:
/// KAM does not lock them and SRM does not draw them (Jev, KEYS_ONLY).
#[test]
fn the_wheels_keys_are_neither_locked_nor_echoed() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h\x1b[2h\x1b[6;10H");
        t.clear_reply();
        assert!(t.wheel_key(KeyEvent::press(KeyCode::Up, Modifiers::NONE)));
        assert_eq!(t.reply(), b"\x1b[A", "sent under KAM");
        support::feed(t, b"\x1b[2l\x1b[12l");
        assert!(t.wheel_key(KeyEvent::press(KeyCode::Up, Modifiers::NONE)));
        assert_eq!(cursor(t), (5, 9), "and not echoed under SRM");
    });
}

/// The echo reads a key's bytes in the protocol the terminal is in: in
/// VT52 Up sends `ESC A`, which is VT52's cursor up.
#[test]
fn the_echo_follows_the_terminal_into_vt52() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[6;10H\x1b[?2l");
        press(t, KeyCode::Up);
        assert_eq!(cursor(t), (4, 9), "VT52's ESC A");
        support::feed(t, b"\x1b<");
        press(t, KeyCode::Up);
        assert_eq!(cursor(t), (3, 9), "and back in ANSI, CSI A");
        assert_eq!(t.unsupported + t.vt52_unsupported, 0);
    });
}

#[test]
fn ris_sets_srm_and_decstr_keeps_it() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[12l\x1b[!p");
        assert!(t.local_echo(), "DECSTR names no SRM");
        support::feed(t, b"\x1bc");
        assert!(!t.local_echo(), "RIS: no local echo, the power-on state");
        support::feed(t, b"\x1b[12l\x1b[12$p");
        assert_eq!(t.reply(), b"\x1b[12;2$y");
    });
}

// ── ?66 DECNKM ──────────────────────────────────────────────────────────────

fn keys(t: &mut Term<'_>, code: KeyCode, mods: Modifiers) -> Vec<u8> {
    t.clear_reply();
    t.key(KeyEvent::press(code, mods));
    t.reply().to_vec()
}

/// `[[source: vt510-decnkm]]`: "This control function works like the
/// DECKPAM and DECKPNM functions. DECNKM is provided mainly for use with the
/// request and report mode (DECRQM/DECRPM) control functions."
#[test]
fn decnkm_is_the_keypad_mode_and_decrqm_reads_it() {
    support::with_corpus(|t| {
        let one = KeyCode::Keypad(KeypadKey::Digit1);
        assert_eq!(keys(t, one, Modifiers::NONE), b"1");
        support::feed(t, b"\x1b[?66h");
        assert_eq!(t.keypad(), KeypadMode::Application);
        assert_eq!(
            keys(t, one, Modifiers::NONE),
            b"\x1bOF",
            "the application column"
        );
        t.clear_reply();
        support::feed(t, b"\x1b[?66$p");
        assert_eq!(t.reply(), b"\x1b[?66;1$y");
        support::feed(t, b"\x1b[?66l\x1b[?66$p");
        assert_eq!(t.reply(), b"\x1b[?66;1$y\x1b[?66;2$y");
        assert_eq!(t.keypad(), KeypadMode::Numeric);
        t.clear_reply();
        support::feed(t, b"\x1b=\x1b[?66$p\x1b>\x1b[?66$p");
        assert_eq!(
            t.reply(),
            b"\x1b[?66;1$y\x1b[?66;2$y",
            "DECKPAM and DECKPNM"
        );
        assert_eq!(t.unsupported_param, 0);
    });
}

// ── ?67 DECBKM ──────────────────────────────────────────────────────────────

/// `[[source: vt510-decbkm]]`: "Default: Delete key ... If DECBKM is set,
/// ⌫ works as a backspace key ... sends a BS character". Control inverts
/// it, xterm's `backarrowKey` ("Pressing the control key toggles this
/// behavior") and the kitty protocol's legacy table (Jev's lean,
/// XTERM_TOGGLE, 0.62/0.70).
#[test]
fn decbkm_chooses_what_the_backarrow_key_sends() {
    support::with_corpus(|t| {
        let bs = KeyCode::Backspace;
        assert_eq!(keys(t, bs, Modifiers::NONE), b"\x7f", "reset: DEL");
        assert_eq!(keys(t, bs, Modifiers::CTRL), b"\x08", "Control: BS");
        support::feed(t, b"\x1b[?67h");
        assert_eq!(keys(t, bs, Modifiers::NONE), b"\x08", "set: BS");
        assert_eq!(keys(t, bs, Modifiers::CTRL), b"\x7f", "Control: DEL");
        support::feed(t, b"\x1b[?2l");
        assert_eq!(keys(t, bs, Modifiers::NONE), b"\x08", "and in VT52");
    });
}

#[test]
fn decbkm_names_the_byte_modify_other_keys_reports() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[>4;2m");
        assert_eq!(
            keys(t, KeyCode::Backspace, Modifiers::CTRL),
            b"\x1b[27;5;8~"
        );
        support::feed(t, b"\x1b[?67h");
        assert_eq!(
            keys(t, KeyCode::Backspace, Modifiers::CTRL),
            b"\x1b[27;5;127~"
        );
    });
}

#[test]
fn decbkm_is_reported_kept_by_decstr_and_reset_by_ris() {
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b[?67$p\x1b[?67h\x1b[?67$p\x1b[!p\x1b[?67$p\x1bc\x1b[?67$p",
        );
        assert_eq!(
            t.reply(),
            b"\x1b[?67;2$y\x1b[?67;1$y\x1b[?67;1$y\x1b[?67;2$y".as_slice()
        );
    });
}

// ── ?1037 ───────────────────────────────────────────────────────────────────

/// `[[source: ctlseqs]]`: "Send DEL from the editing-keypad Delete key", and
/// reset "Send VT220 Remove". A modified Delete stays `CSI 3 ; m ~` and the
/// numeric keypad's Delete is not the editing keypad's, as in xterm's
/// input.c (`[[source: xterm-input-c]]`).
#[test]
fn mode_1037_makes_the_editing_delete_send_del() {
    support::with_corpus(|t| {
        let del = KeyCode::Delete;
        let kp = KeyCode::Keypad(KeypadKey::Delete);
        assert_eq!(keys(t, del, Modifiers::NONE), b"\x1b[3~", "reset: Remove");
        support::feed(t, b"\x1b[?1037h");
        assert_eq!(keys(t, del, Modifiers::NONE), b"\x7f", "set: DEL");
        assert_eq!(
            keys(t, del, Modifiers::SHIFT),
            b"\x1b[3;2~",
            "modified: Remove"
        );
        assert_eq!(
            keys(t, kp, Modifiers::NONE),
            b"\x1b[3~",
            "the keypad's Delete"
        );
        t.clear_reply();
        support::feed(t, b"\x1b[?1037$p\x1b[?1037l\x1b[?1037$p");
        assert_eq!(t.reply(), b"\x1b[?1037;1$y\x1b[?1037;2$y");
        assert_eq!(keys(t, del, Modifiers::NONE), b"\x1b[3~");
    });
}

// ── ?1035 ───────────────────────────────────────────────────────────────────

/// `[[source: ctlseqs]]`: "Use the NumLock key to override the application
/// mode", which `?1035` enables, xterm's `numLock` resource; set at
/// power-on, as that resource is (Jev's lean, SET, 0.66/0.58).
#[test]
fn mode_1035_lets_num_lock_override_the_application_keypad() {
    support::with_corpus(|t| {
        let one = KeyCode::Keypad(KeypadKey::Digit1);
        let enter = KeyCode::Keypad(KeypadKey::Enter);
        support::feed(t, b"\x1b[?1035$p");
        assert_eq!(t.reply(), b"\x1b[?1035;1$y", "set at power-on");
        support::feed(t, b"\x1b=");
        assert_eq!(
            keys(t, one, Modifiers::NUM_LOCK),
            b"1",
            "NumLock: the digit"
        );
        assert_eq!(keys(t, enter, Modifiers::NUM_LOCK), b"\r");
        assert_eq!(
            keys(t, one, Modifiers::NONE),
            b"\x1bOF",
            "without it, DECKPAM"
        );
        support::feed(t, b"\x1b[?1035l");
        assert_eq!(
            keys(t, one, Modifiers::NUM_LOCK),
            b"\x1bOF",
            "reset: DECKPAM"
        );
        support::feed(t, b"\x1b[?1035h\x1b[?2l");
        assert_eq!(keys(t, one, Modifiers::NUM_LOCK), b"1", "and in VT52");
    });
}

/// **A navigation key keeps DECKPAM with NumLock on.** xterm overrides the
/// application keypad only for a keypad key that produced one byte of
/// text, `kd.nbytes == 1` (`[[source: xterm-input-c]]`), and `KP_End`
/// produces none. A keymap that maps Shift with NumLock back to the
/// navigation level sends one, and the Wayland driver reports NumLock on
/// every keypad key.
#[test]
fn a_navigation_key_with_num_lock_keeps_the_application_keypad() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b=");
        let held = Modifiers::NUM_LOCK.with(Modifiers::SHIFT);
        let end = KeyCode::Keypad(KeypadKey::End);
        let home = KeyCode::Keypad(KeypadKey::Home);
        assert_eq!(keys(t, end, Modifiers::NUM_LOCK), b"\x1bOF");
        assert_eq!(keys(t, home, Modifiers::NUM_LOCK), b"\x1bOH");
        assert_eq!(keys(t, end, held), b"\x1b[1;2F", "Shift is still sent");
        assert_eq!(
            keys(t, KeyCode::Keypad(KeypadKey::Digit1), Modifiers::NUM_LOCK),
            b"1",
            "the digit leaves it"
        );
    });
}

/// **A lock is not a modifier the base layer can send.** NumLock and
/// CapsLock have no ctlseqs code, so a key carrying only them is the key
/// unmodified: `CSI A`, not `CSI 1 ; 1 A`, which named a modifier of
/// nothing. xterm maps no lock into its parameter either. The Wayland
/// driver reports both on any key whose type does not consume them.
#[test]
fn a_lock_alone_adds_no_modifier_parameter() {
    support::with_corpus(|t| {
        let locks = Modifiers::NUM_LOCK.with(Modifiers::CAPS_LOCK);
        assert_eq!(keys(t, KeyCode::Up, locks), b"\x1b[A");
        assert_eq!(keys(t, KeyCode::Delete, locks), b"\x1b[3~");
        assert_eq!(
            keys(t, KeyCode::Function(FnKey::new(1).expect("F1")), locks),
            b"\x1bOP"
        );
        assert_eq!(
            keys(t, KeyCode::Up, locks.with(Modifiers::SHIFT)),
            b"\x1b[1;2A"
        );
        support::feed(t, b"\x1b[>4;2m");
        assert_eq!(
            keys(t, ch('a'), locks),
            b"a",
            "modifyOtherKeys takes no lock"
        );
    });
}

#[test]
fn the_key_modes_are_saved_and_restored() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?66;67;1037h\x1b[?1035l\x1b[?66;67;1035;1037s");
        support::feed(t, b"\x1b[?66;67;1037l\x1b[?1035h\x1b[?66;67;1035;1037r");
        support::feed(t, b"\x1b[?66$p\x1b[?67$p\x1b[?1035$p\x1b[?1037$p");
        assert_eq!(
            t.reply(),
            b"\x1b[?66;1$y\x1b[?67;1$y\x1b[?1035;2$y\x1b[?1037;1$y".as_slice()
        );
        assert_eq!(t.unsupported_param, 0);
    });
}

#[test]
fn ris_returns_the_key_modes_to_power_on() {
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b[?67;1037h\x1b[?1035l\x1bc\x1b[?67$p\x1b[?1035$p\x1b[?1037$p",
        );
        assert_eq!(
            t.reply(),
            b"\x1b[?67;2$y\x1b[?1035;1$y\x1b[?1037;2$y".as_slice()
        );
    });
}
