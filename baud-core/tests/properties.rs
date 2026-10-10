//! Generated programs, over an alphabet chosen from the code rather than from
//! a language.
//!
//! **The alphabet is the whole design.** `baud-core2`'s generator has 79
//! operations and reaches the *protocol*; this one has twenty-eight atoms and
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

mod support;

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
    // **DECTCEM, and XTSAVE and XTRESTORE of it**: the only mode atoms in the
    // alphabet, and what the drivers below compare beside the cells. A save
    // or a restore whose sequence a read boundary splits, or a restore that
    // fires where nothing was saved, leaves the cursor shown where it should
    // be hidden or the reverse. A mode that moves the cursor or changes the
    // wrap would put the cache on the screen itself, and both were tried
    // first: `?7l` and `?6h` each broke the two chunk properties at once on
    // defects of the write path's own, not of the cache -- an ASCII write
    // over a wide character's right half at the last column under DECAWM
    // reset, and a base and its mark over a wide character's left half from
    // a new read, each of which leaves the other half on one path and blanks
    // it on another. Recorded; each fix is a write-path change of its own.
    Atom {
        name: "cursor-hide",
        bytes: b"\x1b[?25l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cursor-show",
        bytes: b"\x1b[?25h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "xtsave-dectcem",
        bytes: b"\x1b[?25s",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "xtrestore-dectcem",
        bytes: b"\x1b[?25r",
        stride: 0,
        count: 0,
    },
    // **C1 controls inside control strings, under ISO 8859-1 and under
    // UTF-8.** Each Latin-1 atom enters with `ESC % @` and leaves with
    // `ESC % G`, the shape the VT52 op had in `baud-core2`'s generator, so the
    // byte split can fall on either switch and inside the string. Under
    // Latin-1 the C1 cancels its string and acts; under UTF-8 the same byte is
    // payload, and `osc-c1-utf8` holds that path. The title is compared
    // beside the cells for them: an `OSC` a C1 cancelled, or one it began,
    // changes nothing else.
    //
    // **A bare `ESC % @` atom was tried and dropped.** It leaves every later
    // atom read as Latin-1, where `zwj-family`'s bytes carry `8D`, an RI, and
    // that reached a write-path defect of its own with no control string in
    // it: `ESC [ 2 ; 1 H`, four wide characters, `ESC [ 1 ; 20 H b © ~`, then
    // `the` in the next read, leaves column 0 of row 1 blank where the same
    // bytes in one read keep the `©`. A non-ASCII character wrapping onto a
    // wide character's left half, then ASCII; recorded, and the fix is the
    // write path's.
    Atom {
        name: "latin1-osc-nel",
        bytes: b"\x1b%@\x1b]0;t\x85u\x07\x1b%G",
        stride: 0,
        count: 0,
    },
    // `9B` CSI inside an `OSC`: an SGR that applies to the `u` after it.
    Atom {
        name: "latin1-osc-csi",
        bytes: b"\x1b%@\x1b]0;t\x9b31mu\x07\x1b%G",
        stride: 0,
        count: 0,
    },
    // `9D` OSC inside a DECRQSS: a title instead of a reply.
    Atom {
        name: "latin1-dcs-osc",
        bytes: b"\x1b%@\x1bP$q\x9d2;v\x07\x1b%G",
        stride: 0,
        count: 0,
    },
    // The first one's bytes under UTF-8, where `85` stays in the title.
    Atom {
        name: "osc-c1-utf8",
        bytes: b"\x1b]0;t\x85u\x07",
        stride: 0,
        count: 0,
    },
    // **C1 controls inside escape and control sequences, under ISO 8859-1**:
    // the anywhere rule's other half, each atom entering and leaving Latin-1
    // as the three above do. `85` after a bare `ESC` cancels it and is NEL;
    // `9B` after `ESC (` cancels the designation and opens a `CSI` that
    // colours the `u`; `9D` inside a `CSI`'s parameters cancels it and begins
    // a title, which the drivers compare.
    Atom {
        name: "latin1-esc-nel",
        bytes: b"\x1b%@\x1b\x85u\x1b%G",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "latin1-esc-inter-csi",
        bytes: b"\x1b%@\x1b(\x9b31mu\x1b%G",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "latin1-csi-osc",
        bytes: b"\x1b%@\x1b[1;\x9d2;w\x07\x1b%G",
        stride: 0,
        count: 0,
    },
    // **A BEL inside a DCS body and inside an APC**: payload in the first,
    // ignored in the second, and the end of neither, so the `u` and the `v`
    // after them are body and only the `ESC \` closes each. The DECRQSS
    // answers for a `Pt` of `m BEL u`, which names nothing, and the drivers
    // compare the reply beside the cells. A split can fall between the BEL
    // and the byte after it, where a string the BEL had ended would leave the
    // `u` to be drawn.
    Atom {
        name: "dcs-bel",
        bytes: b"\x1bP$qm\x07u\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "apc-bel",
        bytes: b"\x1b_a\x07v\x1b\\",
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

/// What a driver reads back: every cell, whether the cursor is shown, the
/// title and the reply.
type Seen = (Vec<PackedCell>, bool, Vec<u8>, Vec<u8>);

/// Feed `bytes` in `chunk`-sized reads and return **every cell**, not a row
/// and not a string projection -- whether the cursor is shown, the one mode
/// the alphabet sets, saves and restores, the title, the one string it sets,
/// and the reply, to the one request it makes.
fn drive(bytes: &[u8], chunk: usize) -> Seen {
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
    (
        support::screen(&t),
        t.cursor_visible(),
        t.title().to_vec(),
        t.reply().to_vec(),
    )
}

/// The same program, but each **atom** delivered in its own `feed`.
///
/// **A different mechanism, not a different chunk size.** Splitting by byte is
/// chunk invariance again; splitting by atom makes every atom its own run, so
/// a run of four and four runs of one must agree. That is the class chunk
/// invariance is structurally blind to -- where both chunkings are wrong the
/// same way, which is what the blitter's clipping bug was.
fn drive_atomwise(prog: &[usize]) -> Seen {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    for &i in prog {
        let b = ALPHABET[i].bytes;
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }
    (
        support::screen(&t),
        t.cursor_visible(),
        t.title().to_vec(),
        t.reply().to_vec(),
    )
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

/// **The C1 atoms reach the arms they were added for**, the same control as
/// above: an atom whose C1 never left the string would be payload in both
/// chunkings and green in every property below.
///
/// `latin1-osc-nel`'s `85` cancels the title and is NEL, `latin1-osc-csi`'s
/// `9B` cancels it and colours the `u`, and `latin1-dcs-osc`'s `9D` cancels
/// the DECRQSS and sets a title; in `osc-c1-utf8` the `85` is payload and the
/// title keeps it.
#[test]
fn the_c1_atoms_reach_the_string_arms() {
    let (_, _, title, _) = drive(&bytes_of(&[atom("osc-c1-utf8")]), 4096);
    assert_eq!(title, b"t\x85u", "UTF-8: the 85 is payload");

    fed("latin1-osc-nel", &|t| {
        assert_eq!(t.title(), b"", "NEL: the title was cancelled");
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(1)),
            "NEL ran and `u` printed"
        );
    });
    fed("latin1-osc-csi", &|t| {
        assert_eq!(t.title(), b"", "CSI: the title was cancelled");
        assert_eq!(t.cursor().1, Col::at_index(1), "CSI: `u` printed");
        assert_ne!(
            t.viewport().iter().next().map(|c| c.style),
            Some(baud_domain::StyleId::DEFAULT),
            "CSI: the SGR coloured the `u`"
        );
    });
    fed("latin1-dcs-osc", &|t| {
        assert_eq!(t.title(), b"v", "OSC: the DECRQSS gave way to a title");
        assert!(t.reply().is_empty(), "OSC: the DECRQSS did not answer");
    });
}

/// **The sequence C1 atoms reach theirs**, for the same reason: a C1 the
/// escape or the `CSI` swallowed would be swallowed in both chunkings.
///
/// `latin1-esc-nel`'s `85` cancels the escape and is NEL, so `u` prints at
/// the start of the next line; `latin1-esc-inter-csi`'s `9B` cancels the
/// designation and colours the `u`, and designates nothing; and
/// `latin1-csi-osc`'s `9D` cancels the `CSI` and sets a title.
#[test]
fn the_c1_atoms_reach_the_sequence_arms() {
    fed("latin1-esc-nel", &|t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(1)),
            "ESC 85: NEL ran and `u` printed"
        );
    });
    fed("latin1-esc-inter-csi", &|t| {
        assert_eq!(t.cursor().1, Col::at_index(1), "ESC ( 9B: `u` printed");
        assert_ne!(
            t.viewport().iter().next().map(|c| c.style),
            Some(baud_domain::StyleId::DEFAULT),
            "ESC ( 9B: the SGR coloured the `u`"
        );
        assert_eq!(t.charset_unsupported, 0, "ESC ( 9B: nothing designated");
    });
    fed("latin1-csi-osc", &|t| {
        assert_eq!(t.title(), b"w", "CSI 1 ; 9D: the OSC set the title");
        assert_eq!(t.cursor().1, Col::ZERO, "CSI 1 ; 9D: nothing printed");
    });
}

/// **The BEL atoms reach the string bodies**, for the same reason: a BEL that
/// ended its string would end it in both chunkings, and every property below
/// would stay green with the `u` and the `v` on the screen.
///
/// `dcs-bel`'s BEL is payload, so the `u` is too, nothing is drawn, and the
/// DECRQSS answers invalid for `m BEL u`; `apc-bel`'s BEL is ignored, so the
/// `v` is body and the string is one ignored string; neither rings.
#[test]
fn the_bel_atoms_reach_the_string_bodies() {
    fed("dcs-bel", &|t| {
        assert_eq!(t.cursor().1, Col::ZERO, "DCS: the `u` is payload");
        assert_eq!(t.reply(), b"\x1bP0$r\x1b\\", "DCS: m BEL u is invalid");
        assert_eq!(t.bells, 0, "DCS: no bell");
    });
    fed("apc-bel", &|t| {
        assert_eq!(t.cursor().1, Col::ZERO, "APC: the `v` is body");
        assert_eq!(t.strings_ignored, 1, "APC: one ignored string");
        assert_eq!(t.bells, 0, "APC: no bell");
    });
}

/// Where the atom called `name` sits in the alphabet.
fn atom(name: &str) -> usize {
    ALPHABET
        .iter()
        .position(|a| a.name == name)
        .unwrap_or_else(|| panic!("no atom {name}"))
}

/// The atom called `name` fed alone, in one read, handed to `f`.
fn fed(name: &str, f: &dyn Fn(&Term<'_>)) {
    let bytes = bytes_of(&[atom(name)]);
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    f(&t);
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
        let cells = support::screen(&t);
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
