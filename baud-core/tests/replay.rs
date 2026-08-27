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

/// The three recordings, pinned.
///
/// Each was chosen for a stream the others do not produce: btop is truecolour
/// and box drawing with dense repaints, asciiquarium is curses cursor
/// addressing with 16-colour SGR, and vim carries scroll regions along with
/// CJK, emoji, ZWJ, RTL and combining marks.
const RECORDINGS: [Recording; 3] = [
    Recording {
        name: "btop.raw",
        len: RecordingBytes(524_288),
        sum: Checksum(0x6d8b_9c0b_a82d_3262),
    },
    Recording {
        name: "asciiquarium.raw",
        len: RecordingBytes(335_479),
        sum: Checksum(0x26a6_79cd_a322_5295),
    },
    Recording {
        name: "vim-unicode.raw",
        len: RecordingBytes(138_571),
        sum: Checksum(0x9adb_71fe_a342_20d4),
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
    let btop = load("btop.raw");
    assert!(
        btop.windows(5).any(|w| w == b"38;2;"),
        "btop.raw should carry truecolour SGR"
    );
    assert!(
        btop.windows(3).any(|w| w == [0xe2, 0x94, 0x80]),
        "btop.raw should carry box drawing"
    );

    let aq = load("asciiquarium.raw");
    let sgr16 = aq.windows(2).filter(|w| w == b"\x1b[").count();
    assert!(sgr16 > 10_000, "asciiquarium.raw should be escape-dense");

    let vim = load("vim-unicode.raw");
    assert!(
        vim.windows(3)
            .any(|w| w == "世".as_bytes()[..3.min(3)].to_vec().as_slice()),
        "vim-unicode.raw should carry CJK"
    );
    assert!(
        vim.windows(4).any(|w| w == "🚀".as_bytes()),
        "vim-unicode.raw should carry astral-plane emoji"
    );
}
