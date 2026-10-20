//! **ReGIS reaches its interpreter through `DCS Pn p`, and nothing else
//! does.**
//!
//! `[[source: dec-std-070]]` 070-8 § 8.9.1 introduces ReGIS by "a DEC-private
//! parameterized form of the ANSI escape sequences Device Control String" with
//! the protocol selector "lower case "p"", and its interpreter keeps its state
//! from one envelope to the next unless the parameter's low bit resynchronises.
//! The reports are the visible half here: what the interpreter keeps is read
//! back through `R(P)`, `R(M(=))` and `R(E)`.

use baud_core::*;

mod support;

fn reply_of(bytes: &[u8]) -> Vec<u8> {
    support::with_corpus(|t| {
        support::feed(t, bytes);
        t.reply().to_vec()
    })
}

#[test]
fn a_bare_p_is_regis_and_answers_its_reports() {
    assert_eq!(reply_of(b"\x1bPpP[10,20]R(P)\x1b\\"), b"[10,20]\r");
    assert_eq!(reply_of(b"\x1bP0pP[+5,+6]R(P)\x1b\\"), b"[5,6]\r");
}

#[test]
fn a_dollar_or_plus_p_is_not_regis() {
    // DECRSTS and XTSETTCAP keep their rows: no ReGIS report from either.
    assert_eq!(reply_of(b"\x1bP$pR(P)\x1b\\"), b"");
    assert_eq!(reply_of(b"\x1bP+pR(P)\x1b\\"), b"");
    // A private marker leaves the string ignored, as for a Sixel.
    assert_eq!(reply_of(b"\x1bP?pR(P)\x1b\\"), b"");
}

#[test]
fn an_eight_bit_dcs_is_regis_under_latin1() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b%@");
        support::feed(t, b"\x90pP[3,4]R(P)\x9c");
        assert_eq!(t.reply(), b"[3,4]\r");
    });
}

#[test]
fn the_interpreter_resumes_in_the_next_envelope() {
    // `DCS p`: "Enters ReGIS at the point where ReGIS was last exited"
    // (`[[source: vt3xx-regis-intro]]`) -- here inside a bracket.
    assert_eq!(
        reply_of(b"\x1bPpP[10,2\x1b\\text\x1bPp0]R(P)\x1b\\"),
        b"[10,20]\r"
    );
    // `DCS 1p`: "Enters ReGIS at the command level. ReGIS begins at a new
    // command": the bracket is gone, and with it the move.
    assert_eq!(
        reply_of(b"\x1bPpP[10,2\x1b\\\x1bP1p0]R(P)\x1b\\"),
        b"[0,0]\r"
    );
}

#[test]
fn a_cancelled_envelope_keeps_what_it_did() {
    // A `CAN` ends the string; the position it moved to stays.
    assert_eq!(reply_of(b"\x1bPpP[7,8]\x18\x1bPpR(P)\x1b\\"), b"[7,8]\r");
}

#[test]
fn a_body_split_anywhere_answers_the_same() {
    let body: &[u8] = b"\x1bP1p@:AP[+3,+4]@;P[100,100]@A@AW(M2)P04R(P)R(M(=))R(M(A))\x1b\\";
    let whole = reply_of(body);
    assert_eq!(whole, b"[106,108]\r\"9992,10000\"\r\r");
    for cut in 0..=body.len() {
        let got = support::with_corpus(|t| {
            for part in [&body[..cut], &body[cut..]] {
                t.write_at(BytePos::ZERO, part);
                t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
            }
            t.reply().to_vec()
        });
        assert_eq!(got, whole, "cut at {cut}");
    }
}

#[test]
fn ris_returns_regis_to_power_up() {
    let r = reply_of(b"\x1bPp@:Axyz@;P[9,9]\x1b\\\x1bc\x1bPpR(M(=))R(P)\x1b\\");
    assert_eq!(r, b"\"10000,10000\"\r[0,0]\r");
}

#[test]
fn a_report_that_does_not_fit_is_dropped_whole() {
    support::with_corpus(|t| {
        let mut body = b"\x1bPp".to_vec();
        for _ in 0..2000 {
            body.extend_from_slice(b"R(P)");
        }
        body.extend_from_slice(b"\x1b\\");
        support::feed(t, &body);
        let r = t.reply();
        assert!(r.len() >= 6, "some reports fit");
        // Every report in the buffer is whole.
        assert_eq!(r.len() % b"[0,0]\r".len(), 0);
        assert!(r.chunks(6).all(|c| c == b"[0,0]\r"));
    });
}

#[test]
fn regis_refusals_are_counted() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1bPpS(A(B(C(D(E(F(G)))))))P(E)@:AQ@A@;@A\x1b\\");
        let c = t.regis_counts();
        assert_eq!(c.nesting_overflow, 1);
        assert_eq!(c.stack_underflow, 1);
        assert_eq!(c.entered, 1);
    });
}
