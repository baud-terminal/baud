//! Generated programs, over an alphabet chosen from the code rather than from
//! a language.
//!
//! **The alphabet is the whole design.** `baud-core2`'s generator has 79
//! operations and reaches the *protocol*; this one has fifteen atoms and
//! reaches the **fast paths**, which is where this core's defects have lived.
//! Those paths key on a cross product -- encoded length, width, grapheme class
//! and *run length* -- and two of the entries below exist because a run of
//! four is decoded by a vectorised quad while a run of three takes the scalar
//! tail. A defect in one of those two accumulators is invisible to an
//! alphabet that cannot cross `k == 4`, and one lived there for exactly as
//! long as it took chunk invariance to catch it by hand.
//!
//! **What this suite does not catch, measured rather than assumed.** Four
//! defects were introduced by hand during the work that made this core render
//! like `baud-core2`, and this suite was written to catch them. It catches
//! **none of the four**, which was established by re-introducing each and
//! watching every property stay green:
//!
//! | defect | caught? | why not |
//! |---|---|---|
//! | zero-width counted as narrow in one of two accumulators | **no** | the run gate now **subsumes** it: a run holding a mark is refused on class before the width accumulators decide anything, so the defect is no longer reachable and no input can expose it |
//! | the run gate itself, removed | **no** | *"a flag is one cluster"* is a **specification** fact. Every property here compares the implementation against **itself** -- two chunkings, two feed granularities -- and a missing gate is wrong *consistently*, so no self-differential can see it |
//! | a stranded `#[inline(always)]` | **no** | observationally identical on every input, at any case count |
//! | a fixture that could not fail | **no** | there was no defect behind it |
//!
//! **That is the finding, and it is worth more than the suite.** A property
//! compares a program to itself and so can only find *inconsistency*. A
//! terminal's hardest obligations are agreements with a **document** -- that a
//! regional-indicator pair is one cluster, that `CAN` cancels -- and those need
//! an oracle: `baud-core2` through `baud-bench/tests/differential.rs`, or
//! `GraphemeBreakTest.txt`. It is a direct argument against retiring core2 on
//! the strength of a property suite, and it was cheaper to measure than to
//! argue.
//!
//! What these four **are** is a regression net for the class that has been
//! most productive here historically: a screen that depends on where the reads
//! fell. That class is real, it has produced several defects in this
//! repository, and nothing in `baud-core` was watching for it before.

use baud_core::*;
use proptest::prelude::*;

const ROWS: usize = 50;
/// Narrow on purpose: a twenty-column screen wraps and scrolls under a
/// program of a few dozen atoms, so `RowRot` leaves zero and the addressing
/// paths are exercised. The tests this replaces used **four** rows and read
/// **row 0**, so every scroll-dependent invariant in them was inert.
const COLS: usize = 20;

/// One atom of the alphabet, and what it claims to be.
struct Atom {
    name: &'static str,
    bytes: &'static [u8],
    /// Encoded length of each character, or 0 where the atom is not a run of
    /// one length (ASCII text, control sequences).
    stride: usize,
    /// How many characters, for the atoms that are runs.
    count: usize,
}

/// **The cross product the fast paths key on**, not a sample of a language.
///
/// `run_of` admits a run on encoded length; `put_run` decodes four at a time
/// with a vectorised quad when the stride is three and takes a scalar tail
/// otherwise; the bulk writers demand one width; and the run gate demands
/// class `Other` with no lookback. Every one of those is a separate branch and
/// the entries below cross them.
const ALPHABET: &[Atom] = &[
    // ASCII: the blitter, and long enough to be a real run.
    Atom {
        name: "ascii",
        bytes: b"the quick brown fox",
        stride: 1,
        count: 19,
    },
    Atom {
        name: "ascii-short",
        bytes: b"ab",
        stride: 1,
        count: 2,
    },
    // Two-byte narrow.
    Atom {
        name: "latin1x3",
        bytes: "\u{e9}\u{e9}\u{e9}".as_bytes(),
        stride: 2,
        count: 3,
    },
    // Three-byte wide, k = 4: the vectorised quad.
    Atom {
        name: "wide-quad",
        bytes: "\u{4e16}\u{4e16}\u{4e16}\u{4e16}".as_bytes(),
        stride: 3,
        count: 4,
    },
    // Three-byte wide, k = 3: the scalar tail. The same characters, one fewer.
    Atom {
        name: "wide-three",
        bytes: "\u{4e16}\u{4e16}\u{4e16}".as_bytes(),
        stride: 3,
        count: 3,
    },
    // Three-byte narrow: box drawing, which is what btop is made of.
    Atom {
        name: "narrow-quad",
        bytes: "\u{2500}\u{2500}\u{2500}\u{2500}".as_bytes(),
        stride: 3,
        count: 4,
    },
    // **A zero-width character inside a three-byte run of four.** This is the
    // entry that reaches the vectorised accumulator, and a defect lived in it:
    // zero-width was counted as narrow in the quad loop and not in the scalar
    // tail, so U+0947 got a cell at 4096-byte reads and none at one-byte reads.
    Atom {
        name: "zero-in-quad",
        bytes: "\u{926}\u{947}\u{935}\u{928}".as_bytes(),
        stride: 3,
        count: 4,
    },
    // The same shape at k = 3, which takes the other accumulator.
    Atom {
        name: "zero-in-three",
        bytes: "\u{926}\u{947}\u{935}".as_bytes(),
        stride: 3,
        count: 3,
    },
    // Four-byte, k = 2.
    Atom {
        name: "emoji-pair",
        bytes: "\u{1f600}\u{1f600}".as_bytes(),
        stride: 4,
        count: 2,
    },
    // **A regional-indicator pair**: two adjacent four-byte characters that
    // are one cluster. The only shape that reaches `put_run` with k >= 2 while
    // GB12/13 says the two belong together, and so the only payload that can
    // tell whether the run gate is there.
    Atom {
        name: "flag",
        bytes: "\u{1f1ec}\u{1f1e7}".as_bytes(),
        stride: 4,
        count: 2,
    },
    // A ZWJ family: GB11, and the ZWJ is three bytes between four-byte
    // characters, so the stride run breaks around it.
    Atom {
        name: "zwj-family",
        bytes: "\u{1f468}\u{200d}\u{1f469}".as_bytes(),
        stride: 0,
        count: 0,
    },
    // A base and a combining mark: the cluster store's ordinary case.
    Atom {
        name: "combining",
        bytes: "e\u{301}".as_bytes(),
        stride: 0,
        count: 0,
    },
    // Line movement, so programs scroll.
    Atom {
        name: "crlf",
        bytes: b"\r\n",
        stride: 0,
        count: 0,
    },
    // A rendition, so the style table is exercised.
    Atom {
        name: "sgr",
        bytes: b"\x1b[31m",
        stride: 0,
        count: 0,
    },
    // A cancelled sequence: `CAN` inside a `CSI`. `baud-core2`'s generator
    // excludes CAN and SUB from its C0-inside-CSI set by construction, so no
    // program it builds exercises cancellation at all.
    Atom {
        name: "csi-cancelled",
        bytes: b"\x1b[1;5\x18~",
        stride: 0,
        count: 0,
    },
];

fn program() -> impl Strategy<Value = Vec<usize>> {
    proptest::collection::vec(0..ALPHABET.len(), 0..40)
}

fn bytes_of(prog: &[usize]) -> Vec<u8> {
    let mut v = Vec::new();
    for &i in prog {
        v.extend_from_slice(ALPHABET[i].bytes);
    }
    v
}

fn ws() -> WinSize {
    WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    }
}

/// Feed `bytes` in `chunk`-sized reads and return **every cell**, not a row
/// and not a string projection.
fn drive(bytes: &[u8], chunk: usize) -> Vec<PackedCell> {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
    t.viewport().to_vec()
}

/// The same program, but each **atom** delivered in its own `feed`.
///
/// **A different mechanism, not a different chunk size.** Splitting by byte is
/// chunk invariance again; splitting by atom makes every atom its own run, so
/// a run of four and four runs of one must agree. That is the class chunk
/// invariance is structurally blind to -- where both chunkings are wrong the
/// same way, which is what the blitter's clipping bug was.
fn drive_atomwise(prog: &[usize]) -> Vec<PackedCell> {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    for &i in prog {
        let b = ALPHABET[i].bytes;
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }
    t.viewport().to_vec()
}

/// **The control, and nothing below it is believable until it passes.**
///
/// An atom that silently emits the wrong bytes is indistinguishable from a
/// shape that is never buggy -- the same failure as a search probe that cannot
/// match, and the reason `baud-core2`'s suite runs a positive control over its
/// own generator.
#[test]
fn the_alphabet_emits_what_it_claims() {
    for a in ALPHABET {
        assert!(!a.bytes.is_empty(), "{}: empty", a.name);
        if a.stride == 0 {
            continue;
        }
        assert_eq!(
            a.bytes.len(),
            a.stride * a.count,
            "{}: {} bytes is not {} x {}",
            a.name,
            a.bytes.len(),
            a.stride,
            a.count,
        );
        // Every character really is `stride` bytes long, so the run scanner
        // sees the run this atom claims to be.
        let mut i = 0;
        while i < a.bytes.len() {
            let lead = a.bytes[i];
            let n = match lead {
                0x00..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            assert_eq!(
                n, a.stride,
                "{}: byte {i} leads a {n}-byte character",
                a.name
            );
            i += n;
        }
    }
}

proptest! {
    /// Where the reads fall must not change the screen.
    ///
    /// **Every cell, not row 0**, and over the alphabet above rather than one
    /// hand-written line. The test this replaces compared row 0 of a four-row
    /// screen for a payload with no zero-width character and no run longer
    /// than two -- so it could not reach either defect it was later credited
    /// with being able to catch.
    #[test]
    fn the_screen_does_not_depend_on_where_the_reads_fell(prog in program(), a in 1usize..=64) {
        let b = bytes_of(&prog);
        prop_assert_eq!(drive(&b, a), drive(&b, 4096));
    }

    /// A run and the same characters fed one atom at a time must agree.
    ///
    /// The second mechanism: chunk invariance says two *chunkings* agree and
    /// is silent when both are wrong the same way, which is how a blitter that
    /// clipped at the margin passed it at every read size.
    #[test]
    fn a_program_agrees_with_the_same_atoms_fed_one_at_a_time(prog in program()) {
        let b = bytes_of(&prog);
        prop_assert_eq!(drive(&b, 4096), drive_atomwise(&prog));
    }

    /// **A cell that claims continuations must have them.**
    ///
    /// The one invariant here with no oracle behind it: everything else in
    /// this repo compares two implementations or two chunkings. A cluster
    /// index that has gone stale answers *empty* rather than wrong -- by
    /// design, because the store checks the base -- so this is what turns that
    /// silence into a failure. It is also what discriminates content-keying
    /// from position-keying: `baud-core2`'s marks table cannot pass this
    /// after a `viewport()` call, which is why the index rides in the cell.
    #[test]
    fn every_cell_that_claims_marks_has_them(prog in program()) {
        let b = bytes_of(&prog);
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        t.write_at(BytePos::ZERO, &b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        // At the pitch, not the width: indexed by `COLS`, every probe past
        // row 0 landed inside row 0 -- blank, unmarked, vacuous -- for as
        // long as the pitch was wider than `ROWS * COLS`, and a narrower
        // pitch is what made the probe fire.
        let stride = t.stride().index();
        let cells = t.viewport().to_vec();
        for y in 0..ROWS {
            for x in 0..COLS {
                let c = cells[y * stride + x];
                if !c.flags.is_marked() {
                    continue;
                }
                let n = t.marks(Row::from_index(y), Col::at_index(x)).count();
                prop_assert!(n > 0, "cell ({y},{x}) claims marks and resolves to none");
            }
        }
    }
}
