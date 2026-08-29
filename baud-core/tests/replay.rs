#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! Replaying real capture.
//!
//! `baud-bench/src/payload.rs` says the synthetic payloads want validating
//! against real capture "before it is designed around", and until now that had
//! not happened: every stream baud was tested and measured against was written
//! by the same people who wrote baud. `tests/recordings/` is that capture, and
//! this is what it is for.
//!
//! The assertions are the invariants that hold for *any* input, checked
//! against input nobody here designed. A recording cannot say what the screen
//! should look like -- nothing independently knows -- but it can say that
//! ingesting two and a half million bytes of somebody else's escape sequences
//! never panics, never puts the cursor outside the grid, and produces the same
//! result however the reads are chopped up.

use baud_core::*;
use std::path::PathBuf;

/// Length and a checksum, so a recording that changes is a test failure.
///
/// FNV-1a rather than a real hash: this detects a file being replaced or
/// truncated, which is all it needs to do, and it costs no dependency in a
/// crate that has none. It is not a security property and is not claimed as
/// one.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A recording's size. `ByteLen` is `u16` and holds a read, not a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RecordingBytes(usize);

/// Its FNV-1a, kept distinct from the size so the two cannot be compared to
/// each other -- they are both bare integers otherwise, and this is a table
/// where a transposed pair would still typecheck.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Checksum(u64);

struct Recording {
    name: &'static str,
    len: RecordingBytes,
    sum: Checksum,
}

/// The recordings, pinned.
///
/// Each was chosen for a stream the others do not produce: btop is truecolour
/// and box drawing with dense repaints, asciiquarium is curses cursor
/// addressing with 16-colour SGR, and vim carries scroll regions along with
/// CJK, emoji, ZWJ, RTL and combining marks.
///
/// **The scrolling three were added because `Term::scrolls` said the first
/// three do not scroll** -- btop 0, vim 0, asciiquarium 20 -- while blanking is
/// 73% of `payload::streaming`'s cell writes. Every decision about scrolling
/// rested on a synthetic payload. `cat-scroll` is text with no escapes at all,
/// `git-log` is colour over scrolling text, and `less-pager` repaints by
/// erasing.
///
/// **And the CJK two were added because nothing scrolled *and* carried wide
/// characters.** Counting non-ASCII bytes against wide codepoints separated
/// what looked like one axis into two: btop is 14.5% multi-byte and has **zero
/// wide codepoints** -- box drawing and braille are narrow -- so it exercises
/// the decoder and never the two-cell pair. `vim-unicode` was the only file
/// with wide characters at all, 3,740 of them, and it scrolls zero times.
///
/// So every scroll, erase and line-move in this corpus ran on one-column text,
/// which is the gap the wide-cell orphaning bugs lived in. `cat-cjk` is 81.2%
/// non-ASCII with 69,741 wide codepoints and 4,200 newlines and no escape
/// sequences at all; `less-cjk` is 76.3% with 17,002, in the **alternate
/// screen**, with 1,031 newlines and 294 `ESC M`.
///
/// The `ascii/` and `utf8/` split is by that measurement rather than by
/// intent: a file is in `utf8/` when more than 1% of its bytes are non-ASCII.
/// The three ASCII scrollers carry 400-odd stray high bytes each, 0.1%, which
/// is a stray glyph in a log rather than a decoder workout.
const RECORDINGS: [Recording; 8] = [
    Recording {
        name: "utf8/btop.raw",
        len: RecordingBytes(524_288),
        sum: Checksum(0x6d8b_9c0b_a82d_3262),
    },
    Recording {
        name: "ascii/asciiquarium.raw",
        len: RecordingBytes(335_479),
        sum: Checksum(0x26a6_79cd_a322_5295),
    },
    Recording {
        name: "utf8/vim-unicode.raw",
        len: RecordingBytes(138_571),
        sum: Checksum(0x9adb_71fe_a342_20d4),
    },
    Recording {
        name: "ascii/cat-scroll.raw",
        len: RecordingBytes(448_460),
        sum: Checksum(0xd921_7330_1c22_56d7),
    },
    Recording {
        name: "ascii/git-log.raw",
        len: RecordingBytes(501_185),
        sum: Checksum(0x2695_6f3e_6193_1bf5),
    },
    Recording {
        name: "ascii/less-pager.raw",
        len: RecordingBytes(30_146),
        sum: Checksum(0x289e_1539_54c9_7b45),
    },
    Recording {
        name: "utf8/cat-cjk.raw",
        len: RecordingBytes(327_351),
        sum: Checksum(0xe600_c17a_e73d_7280),
    },
    Recording {
        name: "utf8/less-cjk.raw",
        len: RecordingBytes(84_473),
        sum: Checksum(0xb678_bfaf_8914_b2c9),
    },
];

fn load(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/recordings")
        .join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn drive(bytes: &[u8], chunk: usize, mode: OpMode) -> (Vec<PackedCell>, (usize, usize)) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    let (r, c) = t.cursor();
    (t.viewport().to_vec(), (r.index(), c.index()))
}

/// The recordings are the ones the assertions were written against.
#[test]
fn the_recordings_are_unchanged() {
    for r in &RECORDINGS {
        let d = load(r.name);
        assert_eq!(RecordingBytes(d.len()), r.len, "{} changed length", r.name);
        assert_eq!(
            Checksum(fnv1a(&d)),
            r.sum,
            "{} changed content -- a capture is not reproducible, so this is a \
             replacement rather than a regeneration, and everything below was \
             written against the old bytes",
            r.name
        );
    }
}

/// **Ingesting real capture never panics and leaves the cursor on the grid.**
///
/// The weakest useful claim, against the strongest available input: a megabyte
/// of escape sequences emitted by three programs that have never heard of
/// baud.
#[test]
fn real_capture_ingests_without_panicking() {
    for r in &RECORDINGS {
        let d = load(r.name);
        for mode in [OpMode::Computed, OpMode::Templated, OpMode::Runs] {
            let (vp, (row, col)) = drive(&d, READ_CHUNK, mode);
            assert_eq!(vp.len() % MAX_COLS, 0, "{} in {mode:?}: whole rows", r.name);
            // The positive control. Every assertion here holds trivially for a
            // blank grid, so a replay that quietly did nothing -- a path that
            // stopped feeding, a recording that became empty -- would pass all
            // of them. `Runs` reports representation rather than rendered
            // content, so it is exempt: an unpromoted line is legitimately
            // blank there.
            if mode != OpMode::Runs {
                let painted = vp.iter().filter(|c| **c != PackedCell::EMPTY).count();
                assert!(
                    painted > 100,
                    "{} in {mode:?}: only {painted} cells painted -- the \
                     recording did not reach the grid",
                    r.name
                );
            }
            assert!(
                row < vp.len() / MAX_COLS,
                "{} in {mode:?}: cursor row {row} outside the grid",
                r.name
            );
            assert!(
                col <= MAX_COLS,
                "{} in {mode:?}: cursor column {col}",
                r.name
            );
        }
    }
}

/// **And the result does not depend on where the reads were cut.**
///
/// This is `ingest_is_chunk_invariant` against input the generator could not
/// invent. A real stream splits multi-byte UTF-8, escape sequences and control
/// strings across read boundaries constantly -- vim's capture carries CJK and
/// ZWJ emoji, so a wrong boundary shows up as a different grid rather than a
/// crash.
#[test]
fn real_capture_is_chunk_invariant() {
    for r in &RECORDINGS {
        let d = load(r.name);
        let whole = drive(&d, READ_CHUNK, OpMode::Computed);
        for chunk in [1, 7, 64, 1023] {
            let split = drive(&d, chunk, OpMode::Computed);
            assert_eq!(
                whole.1, split.1,
                "{}: cursor differs at chunk {chunk}",
                r.name
            );
            assert_eq!(
                whole.0, split.0,
                "{}: grid differs at chunk {chunk}",
                r.name
            );
        }
    }
}

/// The recordings exercise what they were chosen for.
///
/// A positive control on the corpus itself. If a recording were replaced by
/// something that no longer carries truecolour or scroll regions, the tests
/// above would keep passing while testing less -- the "probe that cannot fire"
/// shape this repo keeps rediscovering.
#[test]
fn each_recording_carries_what_it_was_chosen_for() {
    let btop = load("utf8/btop.raw");
    assert!(
        btop.windows(5).any(|w| w == b"38;2;"),
        "btop.raw should carry truecolour SGR"
    );
    assert!(
        btop.windows(3).any(|w| w == [0xe2, 0x94, 0x80]),
        "btop.raw should carry box drawing"
    );

    let aq = load("ascii/asciiquarium.raw");
    let sgr16 = aq.windows(2).filter(|w| w == b"\x1b[").count();
    assert!(sgr16 > 10_000, "asciiquarium.raw should be escape-dense");

    let vim = load("utf8/vim-unicode.raw");
    assert!(
        vim.windows(3)
            .any(|w| w == "世".as_bytes()[..3.min(3)].to_vec().as_slice()),
        "vim-unicode.raw should carry CJK"
    );
    assert!(
        vim.windows(4).any(|w| w == "🚀".as_bytes()),
        "vim-unicode.raw should carry astral-plane emoji"
    );

    // The scrolling three, added because `Term::scrolls` said the first three
    // do not scroll.
    let cat = load("ascii/cat-scroll.raw");
    assert!(
        !cat.contains(&0x1b),
        "cat-scroll.raw is text with no escape sequences at all -- that is the \
         whole point of it, and it is what makes it the one recording where \
         the bulk scanner is the only path"
    );
    assert!(cat.iter().filter(|b| **b == b'\n').count() > 5_000);

    let git = load("ascii/git-log.raw");
    assert!(
        git.windows(2).filter(|w| *w == b"\x1b[").count() > 3_000,
        "git-log.raw is colour over scrolling text"
    );
    assert!(git.iter().filter(|b| **b == b'\n').count() > 3_000);

    let less = load("ascii/less-pager.raw");
    assert!(
        less.windows(8).any(|w| w == b"\x1b[?1049h"),
        "less-pager.raw pages in the alternate screen, which is what made it \
         26 MB/s until the rotation covered that screen"
    );
    assert!(
        less.windows(2).filter(|w| *w == b"\x1bM").count() > 100,
        "and it scrolls backwards with RI, which is the other direction"
    );
    assert!(
        !less.windows(3).any(|w| w == b"\x1b[r") && !has_decstbm(&less),
        "and it sets **no** scrolling region: every line move in it is a \
         full-screen scroll. The zero-copy-line-moves plan was gated on the \
         opposite belief for three revisions"
    );

    // The CJK two, added because nothing scrolled *and* carried wide
    // characters. That intersection is the whole reason they exist, so it is
    // asserted rather than described.
    for name in ["utf8/cat-cjk.raw", "utf8/less-cjk.raw"] {
        let d = load(name);
        let non_ascii = d.iter().filter(|b| **b > 0x7f).count();
        assert!(
            non_ascii * 2 > d.len(),
            "{name} should be majority non-ASCII, is {non_ascii}/{}",
            d.len()
        );
        assert!(
            d.iter().filter(|b| **b == b'\n').count() > 500,
            "{name} must scroll -- a UTF-8 recording that does not scroll is \
             what vim already was"
        );
        assert!(
            wide_codepoints(&d) > 5_000,
            "{name} must carry wide codepoints -- btop is 14.5% multi-byte \
             with zero of them, which is how the gap stayed invisible"
        );
    }

    let less_cjk = load("utf8/less-cjk.raw");
    assert!(
        less_cjk.windows(8).any(|w| w == b"\x1b[?1049h"),
        "less-cjk.raw is the alternate screen with wide characters, which is \
         where the erase and scroll bugs lived"
    );
}

/// Does the stream set a scrolling region? `CSI Pt ; Pb r`.
fn has_decstbm(d: &[u8]) -> bool {
    d.windows(2)
        .enumerate()
        .filter(|(_, w)| *w == b"\x1b[")
        .any(|(i, _)| {
            d[i + 2..]
                .iter()
                .take(12)
                .take_while(|b| b.is_ascii_digit() || **b == b';')
                .count()
                > 0
                && d[i + 2..]
                    .iter()
                    .take(12)
                    .find(|b| !b.is_ascii_digit() && **b != b';')
                    == Some(&b'r')
        })
}

/// Codepoints a terminal draws in two columns, counted from the bytes.
///
/// Deliberately a *decode*, not a byte count: `btop.raw` is 14.5% multi-byte
/// and has **zero** wide codepoints, because box drawing and braille are one
/// column each. Counting bytes conflates the two axes, and that conflation is
/// exactly why no measurement in this project saw the CJK path for a year.
fn wide_codepoints(d: &[u8]) -> usize {
    core::str::from_utf8(d)
        .map(|s| {
            s.chars()
                .filter(|c| {
                    let v = u32::from(*c);
                    (0x1100..=0x115F).contains(&v)
                        || (0x2E80..=0xA4CF).contains(&v)
                        || (0xAC00..=0xD7A3).contains(&v)
                        || (0xF900..=0xFAFF).contains(&v)
                        || (0xFF00..=0xFF60).contains(&v)
                        || (0x1F300..=0x1FAFF).contains(&v)
                })
                .count()
        })
        .unwrap_or(0)
}

/// Drive a recording and return everything it made baud say.
///
/// `reply()` is a buffer the embedder is expected to drain, so this drains it
/// per read rather than at the end -- which is also what an embedder does, and
/// what stops a long stream from overflowing 256 bytes.
fn replies_from(bytes: &[u8]) -> (Vec<u8>, u64) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        out.extend_from_slice(t.reply());
        t.clear_reply();
        i += n;
    }
    (out, t.unsupported.count())
}

/// **Real input drives the reply channel, and the tests above ignored it.**
///
/// Every assertion elsewhere in this file reads the grid. A recording also
/// makes baud *speak*, and nothing checked what it said: vim's capture carries
/// three queries, and the replies to them were being dropped on the floor by
/// the very tests meant to validate real capture.
///
/// Two are `CSI 6 n`, answered with CPR. The third is `CSI > c` -- **secondary**
/// device attributes, not primary -- and the right answer to it is silence,
/// which is the interesting half.
#[test]
fn real_capture_drives_the_reply_channel() {
    let (replies, unsupported) = replies_from(&load("utf8/vim-unicode.raw"));

    assert!(!replies.is_empty(), "vim's capture queries the terminal");
    let cprs = replies.windows(1).filter(|w| w[0] == b'R').count();
    assert_eq!(cprs, 2, "two `CSI 6 n` in the recording, two CPR answers");
    for r in replies.split_inclusive(|b| *b == b'R') {
        assert_eq!(r[0], 0x1b, "each reply is its own CSI");
        assert_eq!(r[1], b'[', "and none is an 8-bit introducer here");
    }

    // **The silence is the point.** `CSI > c` is secondary DA. baud does not
    // answer it, because an application parses whatever arrives as the report
    // it asked for and a wrong identity is worse than none -- the same reason
    // primary DA under-claims as a VT102. What it must not do is drop the
    // query without trace, so it is counted.
    assert!(
        !replies.contains(&b'c'),
        "secondary DA must not be answered: {:?}",
        String::from_utf8_lossy(&replies)
    );
    assert!(
        unsupported >= 1,
        "and it must be counted rather than silently dropped"
    );
}

/// The other two recordings ask nothing, and get nothing.
///
/// The control for the test above: if `replies_from` were broken and always
/// returned empty, that test would still pass its silence assertion. This one
/// fails if the harness stopped collecting.
#[test]
fn a_recording_that_asks_nothing_is_answered_with_nothing() {
    for name in ["utf8/btop.raw", "ascii/asciiquarium.raw"] {
        let (replies, _) = replies_from(&load(name));
        assert!(
            replies.is_empty(),
            "{name} contains no queries, so baud should say nothing, got {:?}",
            String::from_utf8_lossy(&replies)
        );
    }
}
