//! Code extension: G0-G3 with the 96-character sets, the locking shifts into
//! GL and GR, the single shifts, and GR under ISO 8859-1.
//!
//! `[[source: ecma35]]` 7.3, 8.3 and 8.4 define the functions,
//! `[[source: dec-std-070]]` 070-3 §3.6 and pp. 3-80/3-81 the receiver, and
//! `[[source: ctlseqs]]` the spellings (`ESC N`, `ESC O`, `ESC n`, `ESC o`,
//! `ESC ~`, `ESC }`, `ESC |`, `ESC - . /`). The forks the texts leave open
//! were put to Jev (decision log lines 900/901 and 906/907) and are named at
//! each test.
//!
//! Every case is fed at read sizes 1, 2, 3, 7 and whole: a read boundary
//! between a single shift and its character is where a half-applied rule
//! would show.

use crate::support;
use baud_core::{GSlot, Owned, Term};
use baud_domain::Charset;
use baud_domain::DecSet;

/// The characters on row 0 up to the cursor, `' '` for a blank.
fn line(t: &Term<'_>) -> String {
    let row = t.row(baud_domain::Row::ZERO).expect("row 0");
    let n = t.cursor().1.index();
    (0..n)
        .map(|x| {
            row[x]
                .content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect()
}

/// Feed `bytes` at each read size and hand each terminal to `f`.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    for chunk in [1, 2, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&t);
    }
}

fn shows(bytes: &[u8], want: &str) {
    each_read_size(bytes, |t| {
        assert_eq!(line(t), want, "{:?}", String::from_utf8_lossy(bytes));
    });
}

// ---- Locking shifts into GL ----------------------------------------------

/// LS2, `ESC n`: G2 into GL until another locking shift (`[[source: ecma35]]`
/// 8.3.1; ctlseqs "Invoke the G2 Character Set as GL"). It was counted.
#[test]
fn ls2_invokes_g2_into_gl() {
    shows(b"\x1b*0\x1bnq\x0fq", "\u{2500}q");
}

/// LS3, `ESC o`: G3 into GL.
#[test]
fn ls3_invokes_g3_into_gl() {
    shows(b"\x1b+0\x1boqq", "\u{2500}\u{2500}");
}

// ---- Single shifts ---------------------------------------------------------

/// SS2, `ESC N`: the next graphic character from G2, then GL as before
/// (`[[source: ecma35]]` 8.4; 070-3 p. 3-80).
#[test]
fn ss2_draws_one_character_from_g2() {
    shows(b"\x1b*0\x1bNqq", "\u{2500}q");
}

/// SS3, `ESC O`, the same from G3.
#[test]
fn ss3_draws_one_character_from_g3() {
    shows(b"\x1b+0\x1bOqq", "\u{2500}q");
}

/// **At power-up G2 and G3 hold the user-preference supplemental set, DEC
/// Supplemental** (070-3 §3.6.4.3, 070-4 p. 14 `upss = DEC_SUPPLEMENTAL_G`;
/// Jev's lean DEC_SUPPLEMENTAL 0.85/0.88, then 0.80/0.82), under UTF-8 too
/// (Jev's lean SAME_IN_BOTH_ENCODINGS): `ESC N A` draws DEC Supplemental's
/// `A`, U+00C1, and `ESC O P` its reserved 5/0, the error character.
#[test]
fn the_single_shifts_reach_dec_supplemental_at_power_up() {
    shows(b"\x1bNA\x1bOPA", "\u{00c1}\u{2426}A");
}

/// **A control between the shift and its character cancels the shift**:
/// 070-3 p. 3-81, "If the next character in the received data stream is not
/// a graphic character in this range, the single shift function is ignored
/// and the received character is processed as if the single shift had not
/// been received" (Jev's lean STD070_CANCEL, 0.56/0.79 then 0.72/0.85).
/// So does an escape sequence, and so does a character outside 2/0-7/15
/// under UTF-8 (decided, AE4).
#[test]
fn a_control_or_sequence_cancels_a_pending_single_shift() {
    shows(b"\x1b*0\x1bN\rqq", "qq");
    shows(b"\x1b*0\x1bN\x1b[mq", "q");
    shows(b"\x1b*0\x1bN\xc3\xa9q", "\u{00e9}q");
}

/// **SO and SI act and keep the shift pending, any number of them** (070-3
/// p. 3-81's exception; its algorithm clears `cancel_single_shift` after
/// each, and Jev decided ANY_NUMBER 0.92/0.96), and SO's GL stays after the
/// shifted character (Jev's lean SO_ACTS).
#[test]
fn shift_out_and_shift_in_between_keep_the_single_shift() {
    shows(b"\x1b*0\x1b)A\x1bN\x0e\x0f\x0eq#", "\u{2500}\u{00a3}");
}

/// SP is in 070's operand range: it writes a space from G2 and consumes the
/// shift. DEL is not a graphic character of a 94-set: it cancels the shift
/// and is ignored, as in ground.
#[test]
fn space_consumes_a_single_shift_and_del_cancels_it() {
    shows(b"\x1b*0\x1bN q", " q");
    shows(b"\x1b*0\x1bN\x7fq", "q");
}

/// **A final after an intermediate is not a single shift**: `ESC SP N`,
/// `ESC ( N`, `ESC # N` end where they end at every split.
#[test]
fn a_final_after_an_intermediate_is_not_a_single_shift() {
    for seq in [&b"\x1b N"[..], b"\x1b(N", b"\x1b#N", b"\x1b O"] {
        let mut prog = b"\x1b*0\x1b+0".to_vec();
        prog.extend_from_slice(seq);
        prog.push(b'q');
        each_read_size(&prog, |t| {
            assert_eq!(line(t).chars().last(), Some('q'), "{seq:?}")
        });
    }
}

// ---- ISO 8859-1: GR, and the 8-bit single shifts ---------------------------

/// **GR holds G2, DEC Supplemental, at power-up** (Jev decided G2, 1.00 and
/// 0.96; the set is Jev's lean DEC_SUPPLEMENTAL): under `ESC % @` `C1` draws
/// `Á`, `D7` DEC Supplemental's `Œ` where ISO 8859-1 has `×`, `A4` its
/// reserved 2/4, the error character; `A0` is the error character and `FF`
/// is ignored with a 94-set in GR (070-3 §3.4.3/§3.4.4; AE8, decided).
#[test]
fn gr_reads_through_g2_under_latin1() {
    shows(
        b"\x1b%@\xc1\xd7\xa4\xa0\xffZ",
        "\u{00c1}\u{0152}\u{2426}\u{2426}Z",
    );
}

/// `8E` and `8F` are SS2 and SS3 under ISO 8859-1, and a GR operand is
/// stripped to its 7-bit code (Jev's lean STRIP_BIT8): `8E F1` draws G2's
/// `q`.
#[test]
fn the_eight_bit_single_shifts_and_a_gr_operand() {
    shows(b"\x1b%@\x1b*0\x8eq\x8f\xf1q", "\u{2500}\u{00f1}q");
    shows(b"\x1b%@\x1b*0\x8e\xf1q", "\u{2500}q");
}

/// LS1R, LS2R, LS3R, `ESC ~ } |`: G1, G2, G3 into GR (`[[source: ecma35]]`
/// 8.3.2), so GR bytes read through the new slot at once.
#[test]
fn the_right_locking_shifts_move_gr() {
    shows(b"\x1b%@\x1b)0\x1b~\xf1", "\u{2500}");
    shows(b"\x1b%@\x1b+B\x1b|\xc1\x1b}\xc1", "A\u{00c1}");
}

/// **A designation into the slot GR holds takes effect at once**
/// (`[[source: ecma35]]` 14.3.2).
#[test]
fn designating_the_slot_gr_holds_changes_gr() {
    shows(b"\x1b%@\x1b*0\xf1", "\u{2500}");
}

// ---- 96-character sets -------------------------------------------------------

/// `ESC - A` designates ISO Latin-1 Supplemental into G1; in GL, 2/0 and
/// 7/15 are characters of the set (`[[source: ecma35]]` 8.3.1): SP is
/// U+00A0 and DEL U+00FF.
#[test]
fn a_96_set_in_gl_draws_space_and_del() {
    shows(b"\x1b-A\x0eA \x7f\x0fA", "\u{00c1}\u{00a0}\u{00ff}A");
}

/// The other five: Latin-2 (`B`), Greek (`F`), Hebrew (`H`), Latin-Cyrillic
/// (`L`) and Latin-5 (`M`), through G2 and G3 and the single shifts.
#[test]
fn every_listed_96_set_is_translated() {
    shows(b"\x1b.B\x1bN!", "\u{0104}");
    shows(b"\x1b.F\x1bNa", "\u{03b1}");
    shows(b"\x1b/H\x1bO`", "\u{05d0}");
    shows(b"\x1b/L\x1bO0", "\u{0410}");
    shows(b"\x1b.M\x1bN]", "\u{0130}");
}

/// A 96-set in GR under ISO 8859-1: `A0` and `FF` are its characters.
#[test]
fn a_96_set_in_gr_draws_a0_and_ff() {
    shows(b"\x1b%@\x1b.A\xa0\xff\xc1", "\u{00a0}\u{00ff}\u{00c1}");
}

/// After a single shift of a 94-set, a DEL with a 96-set in GL is GL's
/// character: the shift is cancelled and the byte processed as in ground.
#[test]
fn del_after_a_94_set_single_shift_is_gls_with_a_96_set() {
    shows(b"\x1b-A\x0e\x1bN\x7f", "\u{00ff}");
}

// ---- DECSC, DECRC and the resets ---------------------------------------------

/// DECSC saves which slot GR invokes (`[[source: vt510-decsc]]`, "Character
/// sets (G0, G1, G2, or G3) currently in GL and GR"; 070-5 §5.13.1 at Level
/// 2), and DECRC restores it.
#[test]
fn decrc_restores_gr() {
    shows(b"\x1b%@\x1b)0\x1b~\x1b7\x1b}\x1b8\xf1", "\u{2500}");
}

/// DECSTR and RIS put G2 back in GR and DEC Supplemental in G2 and G3, and a
/// DECRC with nothing saved does too (vt510-decrc: "the DEC Supplemental
/// Graphic set into GR").
#[test]
fn the_resets_restore_gr_and_the_supplemental_slots() {
    shows(b"\x1b*B\x1b|\x1b[!p\x1b%@\xd7", "\u{0152}");
    shows(b"\x1b*B\x1b|\x1bc\x1b%@\xd7", "\u{0152}");
    shows(b"\x1b%@\x1b*B\x1b|\x1b8\xd7", "\u{0152}");
    each_read_size(b"\x1b*0\x1b[!p", |t| {
        assert_eq!(t.charset_in(GSlot::G2), Charset::Dec(DecSet::Supplemental));
        assert_eq!(t.charset_in(GSlot::G3), Charset::Dec(DecSet::Supplemental));
    });
}

// ---- The checksum -------------------------------------------------------------

fn chk(n: u32) -> String {
    format!("{:04X}", n.wrapping_neg() & 0xffff)
}

/// **A character written through GR or a single shift counts the code DEC
/// STD 070's display stores, `parv[0] & 0x7F`, through its set** (p. 5-140;
/// Jev's lean STORED_7BIT, 0.44/0.70 then 0.48/0.58): DEC Supplemental `D7`
/// drawn `Œ` counts 0x57, SS2's line-drawing `q` 0x71, Latin-1
/// Supplemental's `A` in GL 0x41, and SS2's Greek `a` 0x61.
#[test]
fn the_checksum_counts_the_stored_code_through_gr_and_single_shifts() {
    let cases: [(&str, &[u8], u32); 4] = [
        ("GR DEC Supplemental D7", b"\x1b%@\xd7", 0x57),
        ("SS2 line drawing q", b"\x1b*0\x1bNq", 0x71),
        ("Latin-1 Supplemental A in GL", b"\x1b-A\x0eA\x0f", 0x41),
        ("SS2 Greek a", b"\x1b.F\x1bNa", 0x61),
    ];
    for (name, text, code) in cases {
        let mut prog = text.to_vec();
        prog.extend_from_slice(b"\x1b[1;1;1;1;1;1*y");
        each_read_size(&prog, |t| {
            let want = format!("\x1bP1!~{}\x1b\\", chk(code));
            assert_eq!(t.reply(), want.as_bytes(), "{name}");
        });
    }
}
