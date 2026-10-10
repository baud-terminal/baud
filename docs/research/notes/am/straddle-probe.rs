//! Probe for note AM (XTGETTCAP), committed in docs/research/notes/am/ and not built
//! there. Run in a git-archive copy of baud at 5477bf7 as baud-core/tests/straddle_probe.rs:
//!   cargo test -p baud-core --test straddle_probe -- --nocapture
//!
//! Question: does a DCS whose body arrived in read n run its action in read
//! n+1, when read n+1 carries only the terminator? If so, the largest single
//! action of note AG's one-read bound can open a read with one byte, which is
//! the `floor((L-1)/5)` term of AG's formula.
//!
//! Every read is one `write_at` + `feed` pair, so the read boundaries are
//! exactly where the probe puts them (support::feed would re-chunk).
mod support;
use baud_core::*;

/// The richest rendition, from tests/vt220/dcs.rs (`RICH`): DECRQSS answers
/// it in 111 B.
const RICH: &[u8] = b"\x1b[1;2;3;4:3;5;7;8;9;20;26;51;52;53;60;61;62;63;64;11m\
\x1b[58;2;255;255;255;38;2;255;255;255;48;2;255;255;255m";

fn read(t: &mut Term<'_>, bytes: &[u8]) {
    assert!(bytes.len() <= READ_CHUNK, "one read is at most READ_CHUNK");
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn show(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x1b => "ESC ".to_string(),
            0x20..=0x7e => (c as char).to_string(),
            c => format!("<{c:02X}>"),
        })
        .collect()
}

/// Read n holds the setup and an open DECRQSS; read n+1 holds `second`.
fn two_reads(label: &str, first: &[u8], second: &[u8]) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    read(&mut t, first);
    let after_n = t.reply().to_vec();
    t.clear_reply(); // the app drains after every read (baud-app/src/reads.rs)
    read(&mut t, second);
    let after_n1 = t.reply().to_vec();
    println!(
        "{label}\n  read n   ({:>4} B fed): reply {:>3} B\n  read n+1 ({:>4} B fed: {}): reply {:>3} B: {}",
        first.len(),
        after_n.len(),
        second.len(),
        show(second),
        after_n1.len(),
        show(&after_n1)
    );
}

#[test]
fn a_dcs_body_kept_across_reads_answers_in_the_read_of_its_terminator() {
    println!("== A: DECRQSS, body in read n, terminator alone in read n+1");
    two_reads(
        "A1 7-bit, `ESC P $ q m` | `ESC \\`",
        &[RICH, b"\x1bP$qm"].concat(),
        b"\x1b\\",
    );
    two_reads(
        "A2 7-bit, `ESC P $ q m ESC` | `\\` (ST split inside itself)",
        &[RICH, b"\x1bP$qm\x1b"].concat(),
        b"\\",
    );
    two_reads(
        "A3 Latin-1 C1, `ESC % @` `90 $ q m` | `9C`",
        &[b"\x1b%@".as_slice(), RICH, b"\x90$qm"].concat(),
        b"\x9c",
    );
    two_reads(
        "A4 7-bit, `ESC P $ q m` | `BEL`",
        &[RICH, b"\x1bP$qm"].concat(),
        b"\x07",
    );

    println!(
        "\n== B: a full-length `DCS + q` body (XTGETTCAP's shape) in read n, terminator alone in read n+1"
    );
    // 102 two-character names, `6b62` ("kb"), separated by `;`: 102*4 + 101
    // = 509 bytes, the body bounds.py's per_dcs takes, under OSC_MAX = 512.
    let body: Vec<u8> = vec![&b"6b62"[..]; 102].join(&b";"[..]);
    assert_eq!(body.len(), 509);
    for (label, lead, term) in [
        (
            "B1 Latin-1 C1, `90 + q <509 B>` | `9C`",
            &b"\x1b%@\x90+q"[..],
            &b"\x9c"[..],
        ),
        (
            "B2 7-bit, `ESC P + q <509 B>` | `ESC \\`",
            &b"\x1bP+q"[..],
            &b"\x1b\\"[..],
        ),
    ] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let (ig0, tr0) = (t.strings_ignored, t.string_truncations);
        read(&mut t, &[lead, &body].concat());
        let (ig1, tr1) = (t.strings_ignored, t.string_truncations);
        read(&mut t, term);
        let (ig2, tr2) = (t.strings_ignored, t.string_truncations);
        println!(
            "{label}\n  strings_ignored: start {ig0}, after read n {ig1}, after read n+1 {ig2}\n  string_truncations: start {tr0}, after read n {tr1}, after read n+1 {tr2}"
        );
        assert_eq!(ig1, ig0, "the body alone ends nothing");
        assert_eq!(
            ig2,
            ig0 + 1,
            "the terminator alone ends the 509-byte body in read n+1"
        );
        assert_eq!(tr2, tr0, "509 bytes fit OSC_MAX");
    }

    println!("\n== C: note AG's bound shape, measured on DECRQSS");
    // Read n: an open DECRQSS. Read n+1: its `9C`, then 819 whole
    // `90 $ q m 9C` queries -- 1 + 4095 = READ_CHUNK bytes.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    read(&mut t, &[b"\x1b%@".as_slice(), RICH, b"\x90$qm"].concat());
    assert!(t.reply().is_empty());
    t.clear_reply();
    let mut n1 = vec![0x9cu8];
    for _ in 0..819 {
        n1.extend_from_slice(b"\x90$qm\x9c");
    }
    assert_eq!(n1.len(), READ_CHUNK);
    let ov0 = t.reply_overflows;
    read(&mut t, &n1);
    let sent = t.reply().len();
    let dropped = t.reply_overflows - ov0;
    // Every DECRQSS answer here is the same 111 B, sent whole or dropped whole.
    let answers = sent / 111 + dropped;
    println!(
        "C1 read n+1 = `9C` + 819 x `90 $ q m 9C` ({} B)\n  in the buffer: {sent} B ({} answers of 111 B); dropped and counted: {dropped}\n  answers owed by read n+1: {answers}; bytes owed: {} B\n  AG's formula with a 111 B largest action: 111 + floor((4096-1)/5) x 111 = {} B",
        n1.len(),
        sent / 111,
        answers * 111,
        111 + (4095 / 5) * 111
    );
    assert_eq!(sent % 111, 0);
    assert_eq!(answers, 820, "the straddled answer plus 819");
}
