//! DECSCL, `CSI Pl ; Pc " p`, the conformance level (`[[source:
//! dec-std-070]]` 070-4 DECSCL, `[[source: vt510-decscl]]`), and what reads
//! it: `S8C1T`, DECRQSS `" p`, and `ESC SP L`, `M` and `N` (070-3 §3.8.1).

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 5,
};

fn reply_to(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

const RQSS: &[u8] = b"\x1bP$q\"p\x1b\\";

/// **Level 4 and seven-bit at power-up**, DEC STD 070's `host_state_init`
/// and the VT510's factory default, and the primary DA answer still the
/// VT102's.
#[test]
fn the_terminal_powers_up_at_level_four_seven_bit() {
    support::with_term(&WS, |t| {
        assert_eq!(reply_to(t, RQSS), b"\x1bP1$r64;1\"p\x1b\\");
        assert_eq!(reply_to(t, b"\x1b[c"), b"\x1b[?6c");
    });
}

/// **`Pc` chooses the C1 transmission above Level 1**: 1 seven-bit, 0, 2 or
/// omitted eight-bit -- "0 - default (8-bit C1 controls)" -- and DECRQSS
/// reports both, in the controls it now sends. DA1 does not follow the
/// level.
#[test]
fn decscl_selects_the_level_and_the_c1_transmission() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;1\"p");
        assert_eq!(reply_to(t, RQSS), b"\x1bP1$r62;1\"p\x1b\\");
        support::feed(t, b"\x1b[63\"p");
        assert_eq!(reply_to(t, RQSS), b"\x90\x31$r63;0\"p\x9c");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x9b0n");
        support::feed(t, b"\x1b[64;2\"p");
        assert_eq!(reply_to(t, RQSS), b"\x90\x31$r64;0\"p\x9c");
        assert_eq!(reply_to(t, b"\x1b[c"), b"\x9b?6c", "DA1 is the identity");
    });
}

/// **Level 1 is seven-bit only**: DECSCL 61 sends seven-bit whatever `Pc`
/// says, `S8C1T` is counted and changes nothing, and DECRQSS `" p` gets no
/// answer there, DECRQSS being no Level 1 function.
#[test]
fn level_one_is_seven_bit_only() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b G\x1b[61;2\"p");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
        let counted = t.unsupported;
        support::feed(t, b"\x1b G");
        assert_eq!(t.unsupported, counted + 1, "S8C1T counted at Level 1");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
        assert_eq!(reply_to(t, RQSS), b"");
        support::feed(t, b"\x1b[62\"p\x1b G");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x9b0n", "S8C1T at Level 2");
    });
}

/// "Attempts to select conformance levels that are unsupported by the
/// device will be ignored": 60, 65 and a bare `CSI " p` change nothing and
/// are counted.
#[test]
fn an_unsupported_level_is_ignored_and_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;1\"p\x1b[4h");
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[60\"p\x1b[65;1\"p\x1b[\"p");
        assert_eq!(t.unsupported_param, counted + 3);
        assert_eq!(reply_to(t, RQSS), b"\x1bP1$r62;1\"p\x1b\\");
        assert_eq!(reply_to(t, b"\x1b[4$p"), b"\x1b[4;1$y", "no reset ran");
    });
}

/// **DECSCL is a soft reset**: the modes DECSTR restores go back -- IRM,
/// DECOM, the keyboard lock -- and the screen keeps its text.
#[test]
fn decscl_performs_a_soft_reset_and_keeps_the_screen() {
    support::with_term(&WS, |t| {
        support::feed(t, b"kept\x1b[4h\x1b[?6h\x1b[2h\x1b[62;1\"p");
        assert_eq!(reply_to(t, b"\x1b[4$p"), b"\x1b[4;2$y");
        assert_eq!(reply_to(t, b"\x1b[?6$p"), b"\x1b[?6;2$y");
        assert_eq!(reply_to(t, b"\x1b[2$p"), b"\x1b[2;2$y", "unlocked");
        let row: String = t
            .row(Row::ZERO)
            .expect("row 0")
            .iter()
            .take(4)
            .filter_map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|c| char::from_u32(c.value()))
            })
            .collect();
        assert_eq!(row, "kept");
    });
}

/// **`RIS` returns to Level 4 and seven-bit**, the power-up state, after a
/// DECSCL and an `S8C1T`.
#[test]
fn a_full_reset_returns_to_level_four_seven_bit() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;0\"p\x1bc");
        assert_eq!(reply_to(t, RQSS), b"\x1bP1$r64;1\"p\x1b\\");
        support::feed(t, b"\x1b G\x1bc");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
    });
}

/// **`ESC <` returns to Level 1 and seven-bit**, from the Level 4
/// power-up and from Level 2 and eight-bit: `[[source: dec-std-070]]`
/// 070-0A EXIT VT52 EMULATION MODE (p. A-15), "causing the terminal to
/// return to Level 1 operation", and "Level 1 operation is 7-bits only"
/// (070-3 §3.6.4.1). So `ESC Z` answers, DECRQSS `" p` gets no answer and
/// `S8C1T` is counted.
#[test]
fn leaving_vt52_returns_to_level_one_seven_bit() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2l\x1b<");
        assert_eq!(reply_to(t, b"\x1bZ"), b"\x1b[?6c", "DECID at Level 1");
        assert_eq!(reply_to(t, RQSS), b"", "no DECRQSS at Level 1");
        let counted = t.unsupported;
        support::feed(t, b"\x1b G");
        assert_eq!(t.unsupported, counted + 1, "S8C1T counted at Level 1");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
    });
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;0\"p\x1b[?2l\x1b<");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n", "seven-bit");
        assert_eq!(reply_to(t, RQSS), b"", "Level 1, not 2");
    });
}

/// **`CSI ? 2 l` enters VT52 from every level**, the power-up Level 4
/// included, so `ESC Z` answers `ESC / Z`. 070-0A p. A-14 (Rev AX11, 1985)
/// says the control "is only recognized in Level 1 and Level 2", written
/// when its product table knew only those two, and its own change history
/// (A.9.1) "changed descriptions and algorithms to allow entry from any
/// conformance level"; the VT420 guideline lists DECANM among Level 4's
/// queryable modes, the VT510 and VT520 manuals state no level, and the
/// VT520 enters VT52 "from VT500 mode" (`[[source: vt520-rm]]` 2.10.1), as
/// xterm and vttest do. Jev's lean, ANY_LEVEL (0.56/0.76, then 0.74/0.82).
#[test]
fn vt52_is_entered_from_every_level() {
    for level in [&b"61"[..], b"62", b"63", b"64"] {
        support::with_term(&WS, |t| {
            support::feed(t, &[b"\x1b[", level, b"\"p\x1b[?2l"].concat());
            assert_eq!(reply_to(t, b"\x1bZ"), b"\x1b/Z", "{level:?}");
        });
    }
}

/// **`ESC SP N` and `ESC SP L` designate and invoke at Level 3 and 4**:
/// N puts ASCII back in G0 and G0 in GL, so line drawing designated there
/// is gone; L puts ISO Latin-1 Supplemental in G1 and G1 in GR, so under
/// ISO 8859-1 a GR byte reads through it. At Level 2 they are counted and
/// designate nothing.
#[test]
fn the_announcers_designate_at_level_three_and_above() {
    let first = |t: &Term<'_>| {
        t.row(Row::ZERO).expect("row")[0]
            .content
            .as_codepoint()
            .and_then(|c| char::from_u32(c.value()))
    };
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b(0\x1b N\rq");
        assert_eq!(first(t), Some('q'), "G0 back to ASCII");
        // 0xA1 is Ą in Latin-2, which `ESC . B` and LS2R put in GR, and ¡
        // in Latin-1, which the announcer puts there.
        support::feed(t, b"\x1b%@\x1b.B\x1b}\x1b L\r\xa1");
        assert_eq!(first(t), Some('\u{a1}'), "GR on G1, Latin-1");
    });
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;1\"p\x1b(0");
        let counted = t.unsupported;
        support::feed(t, b"\x1b N\x1b L\x1b M\rq");
        assert_eq!(t.unsupported, counted + 3);
        assert_ne!(first(t), Some('q'), "G0 kept its line drawing");
    });
}

/// Feed `bytes` in reads of `n`, as a pty would split them.
fn feed_in(t: &mut Term<'_>, bytes: &[u8], n: usize) {
    for chunk in bytes.chunks(n) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
}

/// Run `bytes` at every read size from one byte to the whole, and hand each
/// terminal to `check` with the size.
fn every_read_size(bytes: &[u8], check: impl Fn(&mut Term<'_>, usize)) {
    for n in 1..=bytes.len() {
        support::with_term(&WS, |t| {
            feed_in(t, bytes, n);
            check(t, n);
        });
    }
}

fn text(t: &Term<'_>, row: usize) -> String {
    t.row(Row::from_index(row))
        .expect("row")
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// **At Level 1 a Level 2 function is not executed, and is counted**:
/// `[[source: dec-std-070]]` DECSCL note 1, "the normal rule that Level 2
/// functions only work in Level 2 operation". ICH and ECH are "Levels:
/// 2-4", DECTCEM "2-4", DECSTR "2, 3, 4"; at Level 2 each acts.
#[test]
fn level_one_does_not_execute_a_level_two_function() {
    // `abc`, home, ICH 1, forward, ECH 1, DECTCEM reset, LS2.
    let ops = b"abc\r\x1b[@\x1b[C\x1b[X\x1b[?25l\x1bn";
    every_read_size(&[&b"\x1b[61\"p"[..], ops].concat(), |t, n| {
        assert_eq!(
            text(t, 0),
            "abc",
            "read size {n}: nothing inserted or erased"
        );
        assert!(t.cursor_visible(), "read size {n}: DECTCEM ignored");
        assert_eq!(t.unsupported, 3, "read size {n}: ICH, ECH and LS2");
        assert_eq!(t.unsupported_param, 1, "read size {n}: ?25");
    });
    every_read_size(&[&b"\x1b[62;1\"p"[..], ops].concat(), |t, n| {
        assert_eq!(text(t, 0), "  bc", "read size {n}: the control, Level 2");
        assert!(!t.cursor_visible(), "read size {n}");
        assert_eq!((t.unsupported, t.unsupported_param), (0, 0));
    });
    // DECSTR is Level 2's too.
    every_read_size(b"\x1b[?25l\x1b[61\"p\x1b[?25l\x1b[!p", |t, n| {
        assert_eq!(t.unsupported, 1, "read size {n}: DECSTR counted");
    });
}

/// **Each level adds its own**: SU and SD are Level 3's ("3x, 4x
/// (Windowing)"), DECIC and DECCRA Level 4's ("4x", "Level: 4").
#[test]
fn levels_two_three_and_four_add_their_functions() {
    let su = b"a\r\n\x1b[S";
    every_read_size(&[&b"\x1b[62;1\"p"[..], su].concat(), |t, n| {
        assert_eq!(text(t, 0), "a", "read size {n}: no SU at Level 2");
        assert_eq!(t.unsupported, 1);
    });
    every_read_size(&[&b"\x1b[63;1\"p"[..], su].concat(), |t, n| {
        assert_eq!(text(t, 0), "", "read size {n}: SU at Level 3");
    });
    let ic = b"ab\r\x1b['}";
    every_read_size(&[&b"\x1b[63;1\"p"[..], ic].concat(), |t, n| {
        assert_eq!(text(t, 0), "ab", "read size {n}: no DECIC at Level 3");
        assert_eq!(t.unsupported, 1);
    });
    every_read_size(&[&b"\x1b[64;1\"p"[..], ic].concat(), |t, n| {
        assert_eq!(text(t, 0), " ab", "read size {n}: DECIC at Level 4");
    });
}

/// **A control no DEC level defines acts at every level** (Jev,
/// EVERY_LEVEL): CHA, REP and the alternate screen at Level 1.
#[test]
fn an_extension_acts_at_level_one() {
    every_read_size(b"\x1b[61\"pab\x1b[5Gc\x1b[2b", |t, n| {
        assert_eq!(text(t, 0), "ab  ccc", "read size {n}");
        assert_eq!(t.unsupported, 0);
    });
}

/// **SGR at Level 1 folds bold and drops colour**: `CSI 1 ; 31 m` sets
/// bold and leaves the foreground, the 31 counted; `CSI 22 m` is Level 2's
/// and leaves bold on. At Level 2 both act.
#[test]
fn level_one_sgr_keeps_bold_and_drops_colour() {
    let first = |t: &Term<'_>| t.style_of(t.row(Row::ZERO).expect("row")[0]);
    every_read_size(b"\x1b[61\"p\x1b[1;31mA\x1b[22mB", |t, n| {
        let a = first(t);
        assert!(a.attrs().has(Attrs::BOLD), "read size {n}");
        assert_eq!(a.fg(), ColorWord::DEFAULT, "read size {n}: no colour");
        let b = t.style_of(t.row(Row::ZERO).expect("row")[1]);
        assert!(b.attrs().has(Attrs::BOLD), "read size {n}: 22 ignored");
        assert_eq!(t.unsupported_param, 2, "read size {n}: 31 and 22");
    });
    every_read_size(b"\x1b[62;1\"p\x1b[1;31mA", |t, n| {
        assert_ne!(
            first(t).fg(),
            ColorWord::DEFAULT,
            "read size {n}: colour at Level 2"
        );
    });
}

/// **A repeated SGR is Level 1's too.** Under `sgr-table` a plain SGR of
/// eight bytes or more that a read held whole is answered from a table on
/// its second sighting, past the dispatch that drops colour at Level 1, so
/// the table must not answer there. Under default features this holds the
/// parse every SGR takes.
#[test]
fn level_one_drops_colour_from_a_repeated_sgr() {
    let sgr = b"\x1b[01;031;004m";
    let mut bytes = Vec::new();
    for b in [&sgr[..], b"X", sgr, b"Y\x1b[61\"p", sgr, b"A"] {
        bytes.extend_from_slice(b);
    }
    every_read_size(&bytes, |t, n| {
        let a = t.style_of(t.row(Row::ZERO).expect("row")[2]);
        assert!(a.attrs().has(Attrs::BOLD), "read size {n}: bold");
        assert_eq!(a.fg(), ColorWord::DEFAULT, "read size {n}: no colour");
    });
}

/// **SCS at Level 1**: ASCII, line drawing and the NRC sets into G0 and G1
/// only, and no DEC Supplemental (`[[source: dec-std-070]]` SCS note 1 and
/// its table; the VT420's Level 1, "Only the ASCII, national replacement
/// (NRC), and DEC Special Graphic character sets are available").
#[test]
fn level_one_designates_into_g0_and_g1_only() {
    every_read_size(b"\x1b[61\"p\x1b*0\x1b(<\x1b(K\x1b)0\x0eq", |t, n| {
        assert_eq!(t.unsupported, 2, "read size {n}: G2 and DEC Supplemental");
        assert_eq!(text(t, 0), "\u{2500}", "read size {n}: G1 line drawing");
    });
}

/// The reply to `bytes` after `pre`, both fed in reads of `n`.
fn reply_at(pre: &[u8], bytes: &[u8], n: usize) -> Vec<u8> {
    support::with_term(&WS, |t| {
        feed_in(t, pre, n);
        t.clear_reply();
        feed_in(t, bytes, n);
        t.reply().to_vec()
    })
}

/// **Level 1 has no Terminal State Interrogation**: DECRQSS, every string,
/// and DECRQM answer nothing, "Levels: 2x (TSI), 3-4", as the VT420 ignores
/// them (`[[source: vt420-rm]]` Table 4-1); at Level 2 both answer, and a
/// Level 4 mode is not recognised.
#[test]
fn level_one_has_no_terminal_state_interrogation() {
    let one = b"\x1b[61\"p";
    let two = b"\x1b[62;1\"p";
    for n in 1..=12 {
        assert_eq!(reply_at(one, b"\x1bP$qm\x1b\\", n), b"", "read size {n}");
        assert_eq!(reply_at(one, b"\x1bP$qr\x1b\\", n), b"", "read size {n}");
        assert_eq!(reply_at(one, b"\x1b[?6$p", n), b"", "read size {n}");
        assert_eq!(reply_at(two, b"\x1bP$qm\x1b\\", n), b"\x1bP1$r0m\x1b\\");
        assert_eq!(reply_at(two, b"\x1b[?69$p", n), b"\x1b[?69;0$y", "DECLRMM");
        assert_eq!(reply_at(two, b"\x1b[?6$p", n), b"\x1b[?6;2$y");
    }
    support::with_term(&WS, |t| {
        support::feed(t, one);
        let counted = t.unsupported;
        support::feed(t, b"\x1bP$qm\x1b\\");
        assert_eq!(t.unsupported, counted + 1, "the DECRQSS counted");
    });
}

/// **Level 1 under ISO 8859-1 strips the eighth bit before any
/// processing** (`[[source: dec-std-070]]` 070-3 §3.7.5.1), from the byte
/// after the DECSCL that selects it, in the same read or the next; DECSCL
/// 62 ends it for the bytes after it. Under UTF-8 nothing is stripped.
#[test]
fn level_one_strips_the_eighth_bit_under_latin1() {
    every_read_size(b"\x1b%@\xc1\x1b[61\"p\xc1\x1b[62;1\"p\xc1", |t, n| {
        assert_eq!(text(t, 0), "\u{c1}A\u{c1}", "read size {n}");
    });
    // `ESC % @` at Level 1 and `ESC <` under ISO 8859-1 start it too.
    every_read_size(b"\x1b[61\"p\xc1\x1b%@\xc1", |t, n| {
        assert_eq!(text(t, 0), "\u{fffd}A", "read size {n}: ESC % @");
    });
    every_read_size(b"\x1b%@\x1b[?2l\xc1\x1b<\xc1", |t, n| {
        assert_eq!(text(t, 0), "\u{c1}A", "read size {n}: ESC <");
    });
    every_read_size("\x1b[61\"p\u{c1}".as_bytes(), |t, n| {
        assert_eq!(text(t, 0), "\u{c1}", "read size {n}: UTF-8 decoded");
    });
    // An eight-bit CSI arrives as ESC: `9B 5 n` is `ESC 5 n`, no DSR.
    for n in 1..=8 {
        let pre = b"\x1b%@\x1b[61\"p";
        assert_eq!(reply_at(pre, b"\x9b5n", n), b"", "read size {n}");
        assert_eq!(reply_at(pre, b"\x9b[5n", n), b"\x1b[0n", "read size {n}");
        assert_eq!(reply_at(b"\x1b%@", b"\x9b5n", n), b"\x1b[0n", "Level 4");
    }
}

/// **The Level 1 keyboard** (`[[source: dec-std-070]]` 070-6 §6.2.4): the
/// editing keys, Home, End, F5 to F10 and F14 to F20 send nothing; F11, F12
/// and F13 send ESC, BS and LF; F1 is still PF1. Under ISO 8859-1 a key
/// that would send an 8-bit code sends nothing.
#[test]
fn the_level_one_keyboard() {
    let press = |pre: &[u8], code: KeyCode| {
        support::with_term(&WS, |t| {
            support::feed(t, pre);
            t.clear_reply();
            let sent = t.key(KeyEvent::press(code, Modifiers::NONE));
            (sent, t.reply().to_vec())
        })
    };
    let f = |n: u8| KeyCode::Function(FnKey::new(n).expect("a function key"));
    let one = b"\x1b[61\"p";
    for k in [
        KeyCode::Insert,
        KeyCode::Delete,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Home,
        KeyCode::End,
        f(5),
        f(6),
        f(10),
        f(14),
        f(20),
    ] {
        assert_eq!(press(one, k), (false, vec![]), "{k:?} at Level 1");
        assert!(press(b"", k).0, "{k:?} at Level 4");
    }
    assert_eq!(press(one, f(11)), (true, b"\x1b".to_vec()));
    assert_eq!(press(one, f(12)), (true, b"\x08".to_vec()));
    assert_eq!(press(one, f(13)), (true, b"\n".to_vec()));
    assert_eq!(press(one, f(1)), (true, b"\x1bOP".to_vec()));
    assert_eq!(press(one, KeyCode::Up), (true, b"\x1b[A".to_vec()));
    // **Shift and Control as DEC STD 070 has them** (070-6): G11 to G13
    // "transmit nothing ... if either the shift key or control key are
    // depressed" (§6.11); a keypad key, PF1 to PF4 among them, is "not
    // effected in any way by the depression of the Control, Shift or Lock
    // keys" (§6.9); Shift "has no effect" on a cursor key (§6.13.2), which
    // is "dead" with Control (§6.13.1 Note 1). Alt, which no DEC keyboard
    // has, is dropped from them as well (§6.2.1 b; Jev's lean).
    let held = |pre: &[u8], code: KeyCode, mods: Modifiers| {
        support::with_term(&WS, |t| {
            support::feed(t, pre);
            t.clear_reply();
            let sent = t.key(KeyEvent::press(code, mods));
            (sent, t.reply().to_vec())
        })
    };
    let (shift, ctrl) = (Modifiers::SHIFT, Modifiers::CTRL);
    let both = ctrl.with(shift);
    for (n, mods) in [(11, shift), (12, ctrl), (13, both), (11, ctrl)] {
        assert_eq!(held(one, f(n), mods), (false, vec![]), "F{n} {mods:?}");
    }
    assert_eq!(held(b"", f(11), shift), (true, b"\x1b[23;2~".to_vec()));
    for (n, fin) in [(1, b'P'), (4, b'S')] {
        for mods in [shift, ctrl, both] {
            let want = vec![0x1b, b'O', fin];
            assert_eq!(held(one, f(n), mods), (true, want), "F{n} {mods:?}");
        }
    }
    assert_eq!(held(b"", f(1), shift), (true, b"\x1b[1;2P".to_vec()));
    let alt = Modifiers::ALT;
    assert_eq!(held(one, f(1), alt), (true, b"\x1bOP".to_vec()), "Alt");
    assert_eq!(held(one, KeyCode::Up, alt), (true, b"\x1b[A".to_vec()));
    assert_eq!(
        held(b"", f(1), alt),
        (true, b"\x1b[1;3P".to_vec()),
        "Alt, 4"
    );
    let pf1 = KeyCode::Keypad(KeypadKey::Pf1);
    assert_eq!(held(one, pf1, ctrl), (true, b"\x1bOP".to_vec()), "PF1");
    assert_eq!(held(one, KeyCode::Up, shift), (true, b"\x1b[A".to_vec()));
    assert_eq!(
        held(b"\x1b[61\"p\x1b[?1h", KeyCode::Left, shift),
        (true, b"\x1bOD".to_vec()),
        "DECCKM"
    );
    assert_eq!(held(b"", KeyCode::Up, shift), (true, b"\x1b[1;2A".to_vec()));
    for mods in [ctrl, both] {
        assert_eq!(held(one, KeyCode::Down, mods), (false, vec![]), "{mods:?}");
    }
    assert!(held(b"", KeyCode::Down, ctrl).0, "Ctrl-Down at Level 4");
    let app = b"\x1b[61\"p\x1b=";
    for k in [KeypadKey::Digit5, KeypadKey::Enter, KeypadKey::Minus] {
        let alone = held(app, KeyCode::Keypad(k), Modifiers::NONE);
        for mods in [shift, ctrl, both] {
            let got = held(app, KeyCode::Keypad(k), mods);
            assert_eq!(got, alone, "{k:?} {mods:?}");
        }
    }
    // A keypad navigation key that duplicates an arrow sends the arrow's
    // code in both keypad modes, so it is that arrow, dead with Control
    // (Jev's lean, INOPERATIVE).
    for k in [KeypadKey::Up, KeypadKey::Left] {
        let kp = KeyCode::Keypad(k);
        for (pre, mode) in [(&one[..], "DECKPNM"), (&app[..], "DECKPAM")] {
            for mods in [ctrl, both] {
                assert_eq!(
                    held(pre, kp, mods),
                    (false, vec![]),
                    "{k:?} {mode} {mods:?}"
                );
            }
            assert_eq!(
                held(pre, kp, shift),
                held(pre, kp, Modifiers::NONE),
                "{k:?} {mode}"
            );
            assert!(held(pre, kp, Modifiers::NONE).0, "{k:?} {mode}");
        }
    }
    let kp_up = KeyCode::Keypad(KeypadKey::Up);
    assert_eq!(held(one, kp_up, shift), (true, b"\x1b[A".to_vec()), "KP Up");
    assert!(held(b"", kp_up, ctrl).0, "Ctrl-KP Up at Level 4");
    let e = KeyCode::Char(Codepoint::from_char('\u{e9}'));
    assert_eq!(press(b"\x1b%@\x1b[61\"p", e), (false, vec![]), "Latin-1");
    assert_eq!(press(b"\x1b%@", e), (true, vec![0xe9]), "Latin-1, Level 4");
    assert_eq!(press(one, e), (true, "\u{e9}".as_bytes().to_vec()), "UTF-8");
    // modifyFunctionKeys -1 makes a modified key a higher-numbered one, and
    // Level 1 rules the key it becomes: Shift-F1 is F11, ESC; Shift-F4 is
    // F14, inoperative.
    let remap_at = |pre: &[u8], mods: Modifiers, n: u8| {
        support::with_term(&WS, |t| {
            support::feed(t, pre);
            support::feed(t, b"\x1b[>2n");
            t.clear_reply();
            let sent = t.key(KeyEvent::press(f(n), mods));
            (sent, t.reply().to_vec())
        })
    };
    let remap = |mods, n| remap_at(one, mods, n);
    assert_eq!(
        remap(Modifiers::SHIFT, 1),
        (true, b"\x1b".to_vec()),
        "Shift-F1"
    );
    assert_eq!(
        remap(Modifiers::SHIFT, 2),
        (true, b"\x08".to_vec()),
        "Shift-F2"
    );
    assert_eq!(remap(Modifiers::SHIFT, 4), (false, vec![]), "Shift-F4");
    assert_eq!(remap(Modifiers::SHIFT, 10), (false, vec![]), "Shift-F10");
    // Past F20 xterm's made-up `CSI 42 ~` and on is no Level 1 key (Jev).
    assert_eq!(remap(Modifiers::CTRL, 1), (false, vec![]), "Ctrl-F1");
    assert_eq!(
        remap(Modifiers::CTRL.with(Modifiers::SHIFT), 2),
        (false, vec![])
    );
    assert_eq!(remap(Modifiers::SHIFT, 11), (false, vec![]), "Shift-F11");
    assert_eq!(remap(Modifiers::CTRL, 12), (false, vec![]), "Ctrl-F12");
    // Unmodified, F11 is still ESC with the resource at -1.
    assert_eq!(remap(Modifiers::NONE, 11), (true, b"\x1b".to_vec()), "F11");
    // The controls: at Level 4 the same presses send the remapped keys.
    let four = |mods, n| remap_at(b"", mods, n);
    assert_eq!(four(Modifiers::SHIFT, 1), (true, b"\x1b[23~".to_vec()));
    assert_eq!(four(Modifiers::SHIFT, 4), (true, b"\x1b[26~".to_vec()));
    assert_eq!(four(Modifiers::CTRL, 1), (true, b"\x1b[42~".to_vec()));
    // Under DECKPAM a keypad navigation key is the key it duplicates, and
    // Level 1 rules it: no editing keypad, no Home or End.
    let kp = KeyCode::Keypad;
    for k in [
        KeypadKey::PageUp,
        KeypadKey::PageDown,
        KeypadKey::Insert,
        KeypadKey::Delete,
        KeypadKey::Home,
        KeypadKey::End,
    ] {
        assert_eq!(press(b"\x1b[61\"p\x1b=", kp(k)), (false, vec![]), "{k:?}");
    }
    assert_eq!(
        press(b"\x1b=", kp(KeypadKey::PageUp)),
        (true, b"\x1b[5~".to_vec())
    );
    assert!(press(b"\x1b[61\"p\x1b=", kp(KeypadKey::Up)).0, "Up");
}
