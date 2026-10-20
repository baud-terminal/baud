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
/// says, `S8C1T` is counted and changes nothing, and DECRQSS `" p` is
/// invalid there, DECRQSS being no Level 1 function.
#[test]
fn level_one_is_seven_bit_only() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b G\x1b[61;2\"p");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
        let counted = t.unsupported;
        support::feed(t, b"\x1b G");
        assert_eq!(t.unsupported, counted + 1, "S8C1T counted at Level 1");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
        assert_eq!(reply_to(t, RQSS), b"\x1bP0$r\x1b\\");
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
/// (070-3 §3.6.4.1). So `ESC Z` answers, DECRQSS `" p` is invalid and
/// `S8C1T` is counted.
#[test]
fn leaving_vt52_returns_to_level_one_seven_bit() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2l\x1b<");
        assert_eq!(reply_to(t, b"\x1bZ"), b"\x1b[?6c", "DECID at Level 1");
        assert_eq!(
            reply_to(t, RQSS),
            b"\x1bP0$r\x1b\\",
            "no DECRQSS at Level 1"
        );
        let counted = t.unsupported;
        support::feed(t, b"\x1b G");
        assert_eq!(t.unsupported, counted + 1, "S8C1T counted at Level 1");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n");
    });
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[62;0\"p\x1b[?2l\x1b<");
        assert_eq!(reply_to(t, b"\x1b[5n"), b"\x1b[0n", "seven-bit");
        assert_eq!(reply_to(t, RQSS), b"\x1bP0$r\x1b\\", "Level 1, not 2");
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
