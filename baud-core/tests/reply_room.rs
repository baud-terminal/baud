//! **One read can ask more than the reply buffer holds, and every question
//! is still answered, whole and in order.** A terminal answers each query:
//! `[[source: ctlseqs]]` never lets a reply be left out (DECRQSS "xterm
//! responds with DCS 1 $ r Pt ST", CPR "Result is CSI r ; c R"), and
//! `[[source: ecma48]]` 8.3.35's "busy, another DSR must be requested later"
//! is a status DSR reports, not leave to drop an answer. Under
//! `ReplyHold::On` a read stops once its answers leave less room than one
//! control can owe; the embedder sends them and feeds the rest, as it does
//! after a page-width request (`Term::take_held`).
//!
//! The reference is the same bytes fed one at a time with the buffer
//! drained after each, where one byte ends at most one control and its
//! answer always fits; every reading here must equal it. A macrograph too
//! long for that has its commands written out in its place instead.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = support::CORPUS;

/// A rich rendition, whose DECRQSS answer is 75 bytes.
const RICH: &[u8] = b"\x1b[1;2;3;4:3;5;7;8;9;53;58:2::1:2:3;38:2::100:200:255;48:2::10:20:30m";

/// A tab stop at every one of the corpus screen's 200 columns, so a DECTABSR
/// answers in 698 bytes to a five-byte question.
fn every_column_a_stop() -> Vec<u8> {
    let mut b = Vec::new();
    for c in 1..=WS.ws_col {
        b.extend_from_slice(format!("\x1b[1;{c}H\x1bH").as_bytes());
    }
    b.extend_from_slice(b"\x1b[H");
    b
}

/// Every palette entry set, so each `OSC 4 ; n ; ?` is answered.
fn every_entry_set() -> Vec<u8> {
    (0..256)
        .flat_map(|n| format!("\x1b]4;{n};rgb:{:02x}/80/40\x1b\\", n).into_bytes())
        .collect()
}

/// `body`, after `setup`, in reads of `chunk` bytes, as `baud-app` feeds a
/// read (`reads.rs`): fed, its answers sent, and the rest a hold left fed
/// after them. Returns every byte answered, in order, and the overflows.
fn answered(setup: &[u8], body: &[u8], chunk: usize, hold: ReplyHold) -> (Vec<u8>, usize) {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    t.set_reply_hold(hold);
    support::feed(&mut t, setup);
    t.clear_reply();
    let before = t.reply_counts().overflows;
    let mut out = Vec::new();
    for read in body.chunks(chunk) {
        t.write_at(BytePos::ZERO, read);
        let mut next = Some(Rest {
            at: BytePos::ZERO,
            len: ReadLen::from_index(read.len()),
        });
        while let Some(r) = next {
            t.feed(r.at, r.len);
            out.extend_from_slice(t.reply());
            t.clear_reply();
            next = t.take_held();
        }
    }
    (out, t.reply_counts().overflows - before)
}

/// The reference: a byte at a time, drained after each, every answer apart.
fn answers(setup: &[u8], body: &[u8]) -> Vec<Vec<u8>> {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, setup);
    t.clear_reply();
    let mut out = Vec::new();
    for &b in body {
        t.write_at(BytePos::ZERO, &[b]);
        t.feed(BytePos::ZERO, ReadLen::ONE);
        if !t.reply().is_empty() {
            out.push(t.reply().to_vec());
        }
        t.clear_reply();
    }
    assert_eq!(
        t.reply_counts().overflows,
        0,
        "a byte at a time, every answer fits"
    );
    out
}

/// The same, joined.
fn reference(setup: &[u8], body: &[u8]) -> Vec<u8> {
    answers(setup, body).concat()
}

/// The floods: four reads' worth of one question each, or herdr's.
fn floods() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
    let fill = |q: &[u8]| q.repeat(4 * READ_CHUNK / q.len());
    let tabs = every_column_a_stop();
    let palette = every_entry_set();
    let herdr: Vec<u8> = (0..4)
        .flat_map(|_| (0..256).flat_map(|n| format!("\x1b]4;{n};?\x1b\\").into_bytes()))
        .collect();
    vec![
        // CPR from the screen's last cell, asked with the C1 introducer:
        // three bytes a question, nine an answer.
        ("CPR", b"\x1b%@\x1b[50;200H".to_vec(), fill(b"\x9b6n")),
        ("DECRQSS", RICH.to_vec(), fill(b"\x1bP$qm\x1b\\")),
        ("DECTABSR", tabs, fill(b"\x1b[2$w")),
        // herdr's startup: the whole palette, four rounds.
        ("OSC 4", palette, herdr),
        // DECID, the one escape that answers, at Level 1, the level that
        // recognises it; and VT52's identify.
        ("DECID", b"\x1b[61\"p".to_vec(), fill(b"\x1bZ")),
        ("VT52 ESC Z", b"\x1b[?2l".to_vec(), fill(b"\x1bZ")),
    ]
}

/// **Every flood is answered whole and in order, at any read size**, with
/// the hold on and the buffer drained when it stops a read: nothing is
/// dropped, and the answers are the ones a byte at a time gets.
#[test]
fn a_read_of_questions_is_answered_whole_and_in_order() {
    for (name, setup, body) in floods() {
        let want = reference(&setup, &body);
        assert!(
            want.len() > REPLY,
            "{name}: the flood asks more than the buffer holds ({} bytes)",
            want.len()
        );
        for chunk in [READ_CHUNK, 1 << 16, 4093, 7] {
            let (got, lost) = answered(&setup, &body, chunk, ReplyHold::On);
            assert_eq!(lost, 0, "{name} in {chunk}-byte reads: none dropped");
            assert!(
                got == want,
                "{name} in {chunk}-byte reads: {} bytes answered, {} asked",
                got.len(),
                want.len()
            );
        }
    }
}

/// **With the hold off an answer that does not fit is dropped whole and
/// counted**, as before: one read's answers are the reference's with whole
/// answers missing, never a cut one, and each missing answer is counted once.
#[test]
fn with_the_hold_off_an_answer_that_does_not_fit_is_dropped_whole_and_counted() {
    for (name, setup, body) in floods() {
        let want = answers(&setup, &body);
        let (got, lost) = answered(&setup, &body, body.len(), ReplyHold::Off);
        assert!(lost > 0, "{name}: one read overflows with the hold off");
        assert!(got.len() <= REPLY, "{name}: the buffer does not grow");
        let (mut at, mut missing) = (0, 0);
        for a in &want {
            match got[at..].starts_with(a) {
                true => at += a.len(),
                false => missing += 1,
            }
        }
        assert_eq!(at, got.len(), "{name}: every byte sent is a whole answer");
        assert_eq!(missing, lost, "{name}: each missing answer counted once");
    }
}

/// **A ReGIS envelope answers inside its string**, a report to each
/// `R(P)`, so the hold has to stop the read between two of its commands
/// and the rest goes on inside the envelope. A drawing position far out in
/// an imposed coordinate system answers in 20 bytes to four.
#[test]
fn a_regis_envelope_is_held_between_its_reports() {
    let mut body = b"\x1bPpS(A[-9999999,-9999999][9999999,9999999])P[-9999999,-9999999]".to_vec();
    body.extend_from_slice(&b"R(P)".repeat(4000));
    body.extend_from_slice(b"\x1b\\");
    let want = reference(b"", &body);
    assert!(
        want.len() > REPLY,
        "the envelope asks more than the buffer holds"
    );
    for chunk in [1 << 16, READ_CHUNK] {
        let (got, lost) = answered(b"", &body, chunk, ReplyHold::On);
        assert_eq!(lost, 0, "in {chunk}-byte reads none is dropped");
        assert!(
            got == want,
            "in {chunk}-byte reads: {} of {}",
            got.len(),
            want.len()
        );
    }
}

/// **A macrograph playing is held between its reports**, as an envelope
/// is: one host byte, the `A` of `@A`, plays every command the macrograph
/// holds, so the read stops inside the playing and the next byte plays the
/// rest first. Playing a macrograph is its text in its place (070-8
/// § 8.4.4), so the reference is the envelope with the commands written
/// out. `far` puts the drawing position far out, 20 bytes a report;
/// `nested` plays one macrograph four times from another. **An envelope
/// left right after the playing**, by `CAN`, `SUB`, an `ESC` and a final or
/// a `RIS`, still answers every report first: the read held inside the
/// playing goes on with the playing before its first byte.
fn macrograph_cases() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
    let far: &[u8] = b"\x1bPpS(A[-9999999,-9999999][9999999,9999999])P[-9999999,-9999999]";
    let r = |n: usize| b"R(P)".repeat(n);
    let one = [far, b"@:A", &r(2400), b"@;@A\x1b\\"].concat();
    let one_out = [far, &r(2400), b"\x1b\\"].concat();
    // Reports written out between the definition and the playing, so a
    // read that holds them all ends with the buffer full before `@A`.
    let after = [
        b"\x1bPp@:A".as_slice(),
        &r(2000),
        b"@;",
        &r(1700),
        b"@A\x1b\\",
    ]
    .concat();
    let after_out = [b"\x1bPp".as_slice(), &r(1700), &r(2000), b"\x1b\\"].concat();
    let nested = [far, b"@:A", &r(2400), b"@;@:B@A@A@A@A@;@B\x1b\\"].concat();
    let nested_out = [far, &r(4 * 2400), b"\x1b\\"].concat();
    let mut cases = vec![
        ("one", one, one_out),
        ("after reports", after, after_out),
        ("nested", nested, nested_out),
    ];
    let left: [(&'static str, &[u8]); 4] = [
        ("left by CAN", b"\x18"),
        ("left by SUB", b"\x1a"),
        ("left by ESC [", b""),
        ("left by RIS", b"\x1bc"),
    ];
    for (name, by) in left {
        let tail = [by, b"\x1b[6n"].concat();
        cases.push((
            name,
            [far, b"@:A", &r(2400), b"@;@A", &tail].concat(),
            [far, &r(2400), &tail].concat(),
        ));
    }
    cases
}

/// **Every report a macrograph plays is answered whole and in order, at
/// any read size**, with the hold on.
#[test]
fn a_macrograph_playing_is_held_between_its_reports() {
    let mut wrong = Vec::new();
    for (name, body, written_out) in macrograph_cases() {
        let want = reference(b"", &written_out);
        assert!(
            want.len() > REPLY,
            "{name}: the playing owes more than the buffer holds"
        );
        for chunk in [1, 7, READ_CHUNK, 1 << 16] {
            let (got, lost) = answered(b"", &body, chunk, ReplyHold::On);
            if lost != 0 || got != want {
                wrong.push(format!(
                    "{name} in {chunk}-byte reads: {} of {} answered, {lost} dropped",
                    got.len(),
                    want.len()
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// **Why the buffer is not sized to what one read can ask**: four bytes,
/// DECTABSR asked with the C1 introducer under ISO 8859-1, answer 9,139 at
/// the widest screen with a stop at every column, so one read of
/// `READ_CHUNK` bytes can owe 9.36 MB (note AG, 1.5).
#[test]
fn four_bytes_can_ask_for_the_longest_tab_report() {
    let wide = WinSize {
        ws_row: 4,
        ws_col: 2048,
    };
    let mut s = Owned::new(&wide);
    let mut t = Term::new(s.storage(), &wide);
    let mut stops = Vec::new();
    for c in 1..=wide.ws_col {
        stops.extend_from_slice(format!("\x1b[1;{c}H\x1bH").as_bytes());
    }
    support::feed(&mut t, &stops);
    support::feed(&mut t, b"\x1b%@");
    t.clear_reply();
    let q = b"\x9b2$w";
    support::feed(&mut t, q);
    assert_eq!(t.reply().len(), 9139, "the longest DECTABSR");
    assert!(
        t.reply().ends_with(b"2048\x1b\\"),
        "a stop at the last column"
    );
    assert_eq!(READ_CHUNK / q.len() * t.reply().len(), 9_358_336);
}

/// One question a program can ask, or something between two.
const ATOMS: &[&[u8]] = &[
    b"\x1b[6n",
    b"\x1b[c",
    b"\x1bP$qm\x1b\\",
    b"\x1b[2$w",
    b"\x1b]4;7;?\x1b\\",
    b"\x1b]4;200;?\x07",
    b"\x1b[?2048;2048;2048;2048;2048;2048;2048;2048h",
    b"\x1bPpR(P)R(P)R(M(=))\x1b\\",
    b"\x1bP+q544e;636f\x1b\\",
    b"\x1b[?1$p",
    b"\x1b[18t",
    b"\x1b%@\x9b6n\x1b%G",
    b"\x1bZ",
    b"text\r\n",
    RICH,
    // Plays `played_z`'s macrograph.
    b"\x1bPp@Z\x1b\\",
    // Plays it, and leaves the envelope by `CAN`.
    b"\x1bPp@Z\x18",
];

/// The macrograph the last atom plays, defined in the property's setup
/// after the drawing position is put far out: `PLAYED` reports, nearly all
/// the buffer holds, from the one byte that plays it.
fn played_z() -> Vec<u8> {
    [
        b"\x1bPpS(A[-9999999,-9999999][9999999,9999999])P[-9999999,-9999999]@:Z".as_slice(),
        &b"R(P)".repeat(PLAYED),
        b"@;\x1b\\",
    ]
    .concat()
}

/// Reports in the macrograph the property plays: 20 bytes each, 20,000 in
/// all.
const PLAYED: usize = 1000;

proptest::proptest! {
    /// **Where the reads fall and how big they are never changes what is
    /// answered**, with the hold on: any program of questions, fed in reads
    /// of any size up to 64 KiB, answers the reference's bytes and drops
    /// none. Run at `PROPTEST_CASES=20000`.
    #[test]
    fn the_answers_do_not_depend_on_where_the_reads_fell(
        prog in proptest::collection::vec(0..ATOMS.len(), 1..1500),
        chunk in proptest::prop_oneof![1usize..64, 64usize..5000, proptest::strategy::Just(1usize << 16)],
    ) {
        let setup = [every_column_a_stop(), every_entry_set(), played_z()].concat();
        let body: Vec<u8> = prog.iter().flat_map(|&i| ATOMS[i].iter().copied()).collect();
        let want = reference(&setup, &body);
        let (got, lost) = answered(&setup, &body, chunk, ReplyHold::On);
        proptest::prop_assert_eq!(lost, 0);
        proptest::prop_assert!(got == want, "{} answered, {} asked", got.len(), want.len());
    }
}
