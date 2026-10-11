//! `OSC 52` writes: what a program puts on the clipboard reaches the embedder
//! whole, at any read size, and what cannot be honoured is refused whole and
//! counted. Reads stay refused with silence (note V, V11; `tests/xterm/osc.rs`).
//!
//! `[[source: ctlseqs]]` 2156-2180: `OSC 52 ; Pc ; Pd`, `Pd` "a string encoded
//! in base64 (RFC-4648)", `?` a read, and "If the second parameter is neither a
//! base64 string nor ? , then the selection is cleared."
//!
//! **The body spans reads on every real copy.** A yank of a few kilobytes is
//! several `read(2)`s, so the decoder's carry and the collected octets live
//! across `feed`s, and the embedder reads a finished write after each one --
//! which is why a second write that starts in the read the first one ended in
//! must not disturb what the first published.

use baud_core::*;
use baud_domain::{ClipboardBudget, ClipboardContent, ClipboardRefusals, Selection, Selections};

mod support;

/// A budget whose text area holds `text` octets.
fn budget(text: usize) -> ClipboardBudget {
    ClipboardBudget::of_text(text)
}

/// The writes an embedder that polls after every read would see: the bytes
/// of each, `None` for a clear, in order.
type Seen = Vec<(Selections, Option<Vec<u8>>)>;

/// Feed `data` in reads of `chunk` bytes, polling after each read the way the
/// app does, into a term whose clipboard area is `room` (`None`: none).
fn run(data: &[u8], chunk: usize, room: Option<usize>, f: impl FnOnce(&Term, Seen)) {
    let ws = support::CORPUS;
    let owned = Owned::new(&ws);
    let mut owned = match room {
        Some(n) => owned.with_clipboard(budget(n)),
        None => owned,
    };
    let mut t = Term::new(owned.storage(), &ws);
    let mut seen = Vec::new();
    let mut last = t.clipboard_writes();
    let mut at = 0;
    while at < data.len() {
        let n = chunk.min(data.len() - at);
        t.write_at(BytePos::ZERO, &data[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
        if t.clipboard_writes() != last {
            last = t.clipboard_writes();
            let w = t.clipboard().expect("a write was published");
            let bytes = match w.content {
                ClipboardContent::Set(b) => Some(b.to_vec()),
                ClipboardContent::Cleared => None,
            };
            seen.push((w.targets, bytes));
        }
    }
    f(&t, seen);
}

/// RFC 4648 base64, padded, for building bodies.
fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for g in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..g.len()].copy_from_slice(g);
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            let s = (n >> (18 - 6 * i)) & 63;
            out.push(if i <= g.len() {
                A[usize::try_from(s).expect("a sextet")]
            } else {
                b'='
            });
        }
    }
    out
}

fn osc52(pc: &[u8], text: &[u8]) -> Vec<u8> {
    [b"\x1b]52;".as_slice(), pc, b";", &b64(text), b"\x1b\\"].concat()
}

/// Four kilobytes of text that is not periodic in three, so a quantum
/// misplaced across a read boundary changes the bytes.
fn text_4k() -> Vec<u8> {
    let table = b"abcdefghijklmnopqrstuvwxyz0123456789 ,.\n";
    (0..4096usize)
        .map(|i| table[(i * 7 + i / 13) % table.len()])
        .collect()
}

fn clipboard() -> Selections {
    Selections::NONE.push(b'c').resolve()
}

const READS: [usize; 6] = [1, 2, 3, 7, 4096, usize::MAX];

#[test]
fn a_4kib_copy_arrives_whole() {
    // Dropped before V11, and cut at `OSC_MAX` (512 bytes of body) had it
    // been kept.
    let text = text_4k();
    run(&osc52(b"c", &text), usize::MAX, Some(8192), |t, seen| {
        assert_eq!(seen, vec![(clipboard(), Some(text.clone()))]);
        assert_eq!(t.clipboard_writes_refused, 0);
        assert_eq!(
            t.string_truncations, 0,
            "the body does not go through OSC_MAX"
        );
    });
}

#[test]
fn a_copy_is_the_same_at_every_read_size() {
    let text = text_4k();
    for chunk in READS {
        run(&osc52(b"c", &text), chunk, Some(8192), |_, seen| {
            assert_eq!(
                seen,
                vec![(clipboard(), Some(text.clone()))],
                "reads of {chunk}"
            );
        });
    }
}

#[test]
fn a_second_copy_starting_in_the_read_the_first_ended_in_leaves_the_first_intact() {
    // The reviewer's bug (note V, V11 design 2): one area for both, and the
    // second body overwrites the first's bytes after they were announced but
    // before the embedder read them. Every read size where the first ends
    // and the second begins inside one read.
    let first = b"the first copy, long enough to span a quantum or two".to_vec();
    let second = text_4k();
    let data = [osc52(b"c", &first), osc52(b"c", &second)].concat();
    let first_len = osc52(b"c", &first).len();
    for chunk in [first_len + 1, first_len + 5, first_len + 100, 64, 97] {
        run(&data, chunk, Some(8192), |_, seen| {
            assert_eq!(
                seen,
                vec![
                    (clipboard(), Some(first.clone())),
                    (clipboard(), Some(second.clone()))
                ],
                "reads of {chunk}"
            );
        });
    }
}

#[test]
fn a_copy_past_the_area_is_refused_whole_and_counted() {
    let text = text_4k();
    run(&osc52(b"c", &text), 1000, Some(4095), |t, seen| {
        assert_eq!(seen, Vec::new(), "nothing is published, and nothing cut");
        assert_eq!(t.clipboard_writes_refused, 1);
        let r = t.clipboard_refusals();
        assert_eq!(r.body, 1);
        assert_eq!(r.body_len, ReadLen::from_index(4096), "the true length");
        assert_eq!(t.clipboard(), None);
    });
    // Exactly full is not refused.
    run(&osc52(b"c", &text), 1000, Some(4096), |_, seen| {
        assert_eq!(seen.len(), 1);
    });
}

#[test]
fn a_pd_that_is_not_base64_clears_the_selection() {
    // ctlseqs: "neither a base64 string nor ? , then the selection is
    // cleared". Published as a clear, so the embedder empties the clipboard.
    for pd in [&b"not base64!"[..], b"Zg=a", b"Z", b"??"] {
        let data = [b"\x1b]52;c;".as_slice(), pd, b"\x07"].concat();
        run(&data, usize::MAX, Some(64), |t, seen| {
            assert_eq!(seen, vec![(clipboard(), None)], "{pd:?}");
            assert_eq!(t.clipboard_writes_refused, 0);
        });
    }
}

#[test]
fn an_empty_pd_sets_an_empty_selection() {
    run(b"\x1b]52;c;\x07", usize::MAX, Some(64), |_, seen| {
        assert_eq!(seen, vec![(clipboard(), Some(Vec::new()))]);
    });
}

#[test]
fn a_storage_without_the_area_refuses_and_counts() {
    run(&osc52(b"c", b"hello"), usize::MAX, None, |t, seen| {
        assert_eq!(seen, Vec::new());
        assert_eq!(t.clipboard_writes_refused, 1);
        assert_eq!(t.clipboard_refusals().unprovisioned, 1);
    });
}

#[test]
fn a_cancelled_body_publishes_nothing() {
    // CAN, SUB and ESC abandon a control string (`[[source: williams]]`);
    // nothing of it is published, and the next copy is whole.
    for cancel in [&b"\x18"[..], b"\x1a", b"\x1b[m"] {
        let data = [
            b"\x1b]52;c;aGVsbG8gd29y".as_slice(),
            cancel,
            &osc52(b"c", b"next"),
        ]
        .concat();
        for chunk in [1, 5, usize::MAX] {
            run(&data, chunk, Some(64), |t, seen| {
                assert_eq!(
                    seen,
                    vec![(clipboard(), Some(b"next".to_vec()))],
                    "{cancel:?} at {chunk}"
                );
                assert_eq!(t.clipboard_writes_refused, 0);
            });
        }
    }
}

#[test]
fn the_selections_named_are_the_selections_published() {
    for pc in [&b"c"[..], b"p", b"s", b"q", b"0", b"7", b"cp", b""] {
        let want = pc
            .iter()
            .fold(Selections::NONE, |s, &b| s.push(b))
            .resolve();
        run(&osc52(pc, b"x"), usize::MAX, Some(64), |_, seen| {
            assert_eq!(seen, vec![(want, Some(b"x".to_vec()))], "{pc:?}");
        });
    }
    // Empty is `s 0`.
    run(&osc52(b"", b"x"), usize::MAX, Some(64), |_, seen| {
        assert!(seen[0].0.contains(Selection::Select));
    });
}

#[test]
fn a_pc_naming_nothing_is_refused() {
    run(&osc52(b"xz", b"x"), usize::MAX, Some(64), |t, seen| {
        assert_eq!(seen, Vec::new());
        assert_eq!(t.clipboard_refusals().no_selection, 1);
        assert_eq!(t.clipboard_writes_refused, 1);
    });
}

#[test]
fn both_terminators_end_a_copy() {
    let bel = [b"\x1b]52;c;".as_slice(), &b64(b"bel"), b"\x07"].concat();
    run(&bel, usize::MAX, Some(64), |_, seen| {
        assert_eq!(seen, vec![(clipboard(), Some(b"bel".to_vec()))]);
    });
    run(&osc52(b"c", b"st"), usize::MAX, Some(64), |_, seen| {
        assert_eq!(seen, vec![(clipboard(), Some(b"st".to_vec()))]);
    });
}

#[test]
fn a_read_is_still_silent_and_counted() {
    run(b"\x1b]52;c;?\x07", usize::MAX, Some(64), |t, seen| {
        assert_eq!(seen, Vec::new());
        assert_eq!(t.clipboard_reads_refused, 1);
        assert!(t.reply().is_empty());
    });
}

#[test]
fn a_long_title_is_still_cut_at_osc_max() {
    // The clipboard route leaves every other OSC on the 512-byte body.
    let title = vec![b'T'; 600];
    let data = [b"\x1b]2;".as_slice(), &title, b"\x07"].concat();
    run(&data, 7, Some(64), |t, _| {
        assert_eq!(t.title().len(), 512 - 2);
        assert_eq!(t.string_truncations, 600 - 510);
    });
}

#[test]
fn a_published_copy_survives_the_strings_after_it() {
    let data = [
        osc52(b"c", b"kept"),
        b"\x1b]2;title\x07\x1b]52;c;?\x07text".to_vec(),
    ]
    .concat();
    run(&data, 3, Some(64), |t, seen| {
        assert_eq!(seen.len(), 1);
        assert_eq!(
            t.clipboard().map(|w| w.content),
            Some(ClipboardContent::Set(b"kept"))
        );
    });
}

/// **Under ISO 8859-1 an `OSC 52` spelled in GR is the one spelled in GL**:
/// `[[source: dec-std-070]]` 3.5.4.5, "GR (8-bit) graphic characters in APC,
/// OSC, and PM control strings will be treated as their 7-bit equivalent (the
/// eighth bit will be ignored)", and `[[source: ecma48]]` §9 for "the contents
/// of the command string". So `B5 B2 BB` is `52;`, which routes the body to
/// the clipboard, and `aGk=` in GR is `hi`. The body kept its GR bytes, so
/// the `52;` was not seen and the write was a refused `OSC`. Every read size
/// in `READS`; the GL spelling first, as the control.
#[test]
fn an_osc_52_spelled_in_gr_is_the_one_spelled_in_gl_under_latin1() {
    let gl = [b"\x1b%@".as_slice(), &osc52(b"c", b"hi")].concat();
    let gr: Vec<u8> = gl
        .iter()
        .enumerate()
        .map(|(i, b)| match (i >= 5 && i < gl.len() - 2, *b) {
            (true, v @ 0x21..=0x7e) => v | 0x80,
            (_, v) => v,
        })
        .collect();
    assert_eq!(&gr[5..10], b"\xb5\xb2\xbb\xe3\xbb", "52;c; in GR");
    for (what, data) in [("GL", &gl), ("GR", &gr)] {
        for chunk in READS {
            run(data, chunk, Some(64), |_, seen| {
                assert_eq!(
                    seen,
                    vec![(clipboard(), Some(b"hi".to_vec()))],
                    "{what}, reads of {chunk}"
                );
            });
        }
    }
}

#[test]
fn the_refusals_start_at_none() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_clipboard(budget(16));
    let t = Term::new(owned.storage(), &ws);
    assert_eq!(t.clipboard_refusals(), ClipboardRefusals::NONE);
    assert_eq!(t.clipboard(), None);
}
