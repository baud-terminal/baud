//! `OSC 52`'s `Pc ; Pd`, streamed, against `[[source: ctlseqs]]` 2156-2180.

use baud_domain::{Base64Step, CutBuffer, Osc52, Osc52End, Selection, Selections};

/// Every byte of `body` (what follows `52;`), then the terminator: what it
/// asked for, and the octets it released.
fn run(body: &[u8]) -> (Osc52End, Vec<u8>) {
    let mut o = Osc52::NEW;
    let mut out = Vec::new();
    for &b in body {
        if let Base64Step::Octets(x) = o.push(b) {
            out.extend_from_slice(x.as_slice());
        }
    }
    let (end, tail) = o.finish();
    out.extend_from_slice(tail.as_slice());
    (end, out)
}

fn targets(pc: &[u8]) -> Selections {
    pc.iter()
        .fold(Selections::NONE, |s, &b| s.push(b))
        .resolve()
}

#[test]
fn every_pc_character_names_its_selection() {
    // "c , p , q , s , 0 , 1 , 2 , 3 , 4 , 5 , 6 , and 7 ... for clipboard,
    // primary, secondary, select, or cut-buffers 0 through 7 respectively".
    let cuts = [
        CutBuffer::B0,
        CutBuffer::B1,
        CutBuffer::B2,
        CutBuffer::B3,
        CutBuffer::B4,
        CutBuffer::B5,
        CutBuffer::B6,
        CutBuffer::B7,
    ];
    let mut all: Vec<(u8, Selection)> = vec![
        (b'c', Selection::Clipboard),
        (b'p', Selection::Primary),
        (b'q', Selection::Secondary),
        (b's', Selection::Select),
    ];
    for (digit, cut) in (b'0'..=b'7').zip(cuts) {
        all.push((digit, Selection::CutBuffer(cut)));
    }
    for &(b, want) in &all {
        let t = targets(&[b]);
        for &(_, other) in &all {
            assert_eq!(
                t.contains(other),
                other == want,
                "{} names {other:?}",
                b as char
            );
        }
    }
}

#[test]
fn an_empty_pc_is_s_0() {
    // "If the parameter is empty, xterm uses s 0".
    let t = targets(b"");
    assert!(t.contains(Selection::Select));
    assert!(t.contains(Selection::CutBuffer(CutBuffer::B0)));
    assert!(!t.contains(Selection::Clipboard));
    assert!(t.names_any());
}

#[test]
fn a_pc_of_unknown_characters_names_nothing() {
    // Not empty, so not `s 0`; and nothing in it is in ctlseqs' set.
    let t = targets(b"xz");
    assert!(!t.names_any());
    assert!(!t.contains(Selection::Select));
    // A known character among unknown ones still names its selection.
    assert!(targets(b"xc").contains(Selection::Clipboard));
}

#[test]
fn pc_names_several_at_once() {
    let t = targets(b"cp");
    assert!(t.contains(Selection::Clipboard) && t.contains(Selection::Primary));
    assert!(!t.contains(Selection::Select));
}

#[test]
fn a_base64_pd_is_a_write_of_its_octets() {
    assert_eq!(
        run(b"c;aGVsbG8="),
        (Osc52End::Write(targets(b"c")), b"hello".to_vec())
    );
    // Unpadded: the decoder's choice, `Base64::finish`.
    assert_eq!(
        run(b"c;aGVsbG8"),
        (Osc52End::Write(targets(b"c")), b"hello".to_vec())
    );
}

#[test]
fn an_empty_pd_is_a_write_of_nothing() {
    // Base64 of nothing is the empty string (RFC 4648 section 10, `BASE64("")
    // = ""`), so an empty `Pd` is base64 and the selection becomes empty.
    assert_eq!(run(b"c;"), (Osc52End::Write(targets(b"c")), Vec::new()));
}

#[test]
fn a_question_mark_is_a_read() {
    assert_eq!(run(b"c;?"), (Osc52End::Read(targets(b"c")), Vec::new()));
    assert_eq!(run(b";?"), (Osc52End::Read(targets(b"")), Vec::new()));
}

#[test]
fn neither_base64_nor_a_question_mark_clears() {
    // "If the second parameter is neither a base64 string nor ? , then the
    // selection is cleared."
    for pd in [
        &b"??"[..],
        b"?Zm9v",
        b"Zm9v?",
        b"hello world",
        b"Zm9v;Zm9v",
        b"Z",
    ] {
        let body = [b"c;".as_slice(), pd].concat();
        assert_eq!(run(&body).0, Osc52End::Clear(targets(b"c")), "{pd:?}");
    }
}

#[test]
fn no_separator_after_pc_is_not_the_form() {
    assert_eq!(run(b"c").0, Osc52End::Malformed);
    assert_eq!(run(b"").0, Osc52End::Malformed);
}
