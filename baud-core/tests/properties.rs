//! Generated programs, over an alphabet chosen from the code rather than from
//! a language.
//!
//! **The alphabet is the whole design.** `baud-core2`'s generator had 79
//! operations and reached the *protocol*; this one has thirty atoms and
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
//! an oracle. When this was written that was `baud-core2` through
//! `baud-bench/tests/differential.rs`, or `GraphemeBreakTest.txt`, and it was
//! a direct argument against retiring core2 on the strength of a property
//! suite, cheaper to measure than to argue. core2 was retired in 2538310,
//! with its renderings frozen in `tests/golden.rs`; the oracles now are those,
//! `GraphemeBreakTest.txt` (`tests/encoding/graphemes.rs`), and the two
//! external differentials, `baud-difftest` against alacritty and
//! `baud-sfte-difftest` against sfte.
//!
//! What these four **are** is a regression net for the class that has been
//! most productive here historically: a screen that depends on where the reads
//! fell. That class is real, it has produced several defects in this
//! repository, and nothing in `baud-core` was watching for it before.

mod support;

use baud_core::*;
use proptest::prelude::*;
use std::collections::BTreeSet;
use support::Form;

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
    // Three-byte private use, k = 4: one column under either ambiguous-width
    // policy where the box drawing above takes two under `Wide`.
    Atom {
        name: "pua-quad",
        bytes: "\u{e0b0}\u{e0b0}\u{e0b0}\u{e0b0}".as_bytes(),
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
    // excluded CAN and SUB from its C0-inside-CSI set by construction, so no
    // program it built exercised cancellation at all.
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
    // be hidden or the reverse. The modes that move the cursor or change the
    // wrap are the atoms after these four.
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
    // **The modes that move the cursor or change the wrap, and the moves
    // that reach a written row.** `?7l` and `?6h` were tried with the four
    // above and dropped: each broke both chunk properties on a write-path
    // defect -- a write over one half of a wide character, from a read that
    // began on its row or after a move onto it, kept the other half on one
    // path and blanked it on another. `Paint`'s mark of how far the cursor's
    // row is written was zero at every read and kept stale on most row
    // changes (`tests/wide_halves.rs`). Fixed, they are here, with `CUP` home
    // and `CUU` so a program lands on rows it has written. Back in, `?7l`
    // found two more at once, `[?7l, ascii, e U+0301]`: a letter written into
    // the last column under `DECAWM` reset leaves the cursor on it, so a mark
    // after it attached to the column before, and a run ending at the margin
    // left the cursor past it where one character did not
    // (`tests/margin_pin.rs`).
    Atom {
        name: "decawm-reset",
        bytes: b"\x1b[?7l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decawm-set",
        bytes: b"\x1b[?7h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decom-set",
        bytes: b"\x1b[?6h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decom-reset",
        bytes: b"\x1b[?6l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cup-home",
        bytes: b"\x1b[H",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cuu",
        bytes: b"\x1b[A",
        stride: 0,
        count: 0,
    },
    // **The bare encoding switches.** `ESC % @` leaves every later atom read
    // as ISO 8859-1, where `zwj-family`'s bytes carry `8D`, an RI. It was
    // tried and dropped for the same defect: `ESC [ 2 ; 1 H`, four wide
    // characters, `ESC [ 1 ; 20 H b © ~the` lost the `©` or kept a tail
    // depending on the reads, a non-ASCII character wrapping onto a wide
    // character's lead. `ESC % G` comes back.
    Atom {
        name: "latin1-enter",
        bytes: b"\x1b%@",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "utf8-enter",
        bytes: b"\x1b%G",
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
    // The bare switches are the two atoms after these.
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
    // **A format effector and a `DEL` inside a DCS body**: the `LF` is put
    // to the DECRQSS handler, which gives it no meaning, and the `DEL` is
    // ignored by the graph, so the `Pt` is `m` and the request answers. A
    // split can fall on either side of each byte; an `LF` that ran would
    // move the cursor, and a `DEL` that stayed would make the answer invalid.
    Atom {
        name: "dcs-lf-del",
        bytes: b"\x1bP$q\nm\x7f\x1b\\",
        stride: 0,
        count: 0,
    },
    // **An introducer's byte after an intermediate, which is a final**:
    // `ESC ( ]` designates a set baud does not name and the `u` is drawn,
    // and `ESC ( % P` the same past two intermediates (`[[source:
    // williams]]`'s `escape intermediate` dispatches `30-7E`, 224). A split
    // before the `]` opened an `OSC` that took the `u` and every atom after
    // it, where one read designated through `fast_esc`.
    Atom {
        name: "esc-inter-introducer",
        bytes: b"\x1b(]u\x1b(%Pv",
        stride: 0,
        count: 0,
    },
    // **A guarded area, and an erase over it**: SPA, `ab`, EPA guards the
    // positions from SPA's through the cursor, and EL 1 spares them under
    // ERM reset. A split can fall between SPA and EPA, where the open start
    // is all that carries across.
    Atom {
        name: "spa-epa-el",
        bytes: b"\x1bVab\x1bW\x1b[1K",
        stride: 0,
        count: 0,
    },
    // **ERM set for one erase**: EL 2 erases a guarded position and keeps
    // its guard, then ERM is reset again.
    Atom {
        name: "erm-el",
        bytes: b"\x1b[6h\x1b[2K\x1b[6l",
        stride: 0,
        count: 0,
    },
    // **DECSCA and the selective erases**: a protected `P`, then DECSEL 1
    // and DECSED 0 from the cursor, which make the erasable characters
    // spaces in their own rendition and leave empty positions.
    Atom {
        name: "decsca-decsel",
        bytes: b"\x1b[1\"qP\x1b[0\"q\x1b[?1K\x1b[?0J",
        stride: 0,
        count: 0,
    },
    // **SM and RM with a list**: IRM second in `CSI 20 ; 4 h`, so the `X`
    // is inserted only if the whole list applies, then both reset.
    Atom {
        name: "sm-list",
        bytes: b"\x1b[20;4hX\x1b[20;4l",
        stride: 0,
        count: 0,
    },
    // **ED, EL and the selective erases with a list**: EL `1 ; 0` erases
    // either side of the cursor only if the second parameter applies, and
    // DECSEL `0 ; 1` the same over erasable characters. A `5` between them
    // is skipped and counted.
    Atom {
        name: "el-list",
        bytes: b"\x1b[1;5;0K",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsel-list",
        bytes: b"\x1b[?0;1K",
        stride: 0,
        count: 0,
    },
    // **GR bytes in a DCS prefix, under ISO 8859-1**: `90`, then `B1 BB B2`
    // for the parameters `1;2`, `A4` for `$` and `F1` for `q`, so the string
    // is a DECRQSS for `m`, closed by `9C`. Each GR byte is read as its
    // 7-bit byte, and a split can fall between any two of them, where the
    // node a prefix byte left the parser in is all that carries across.
    Atom {
        name: "latin1-dcs-gr",
        bytes: b"\x1b%@\x90\xb1\xbb\xb2\xa4\xf1m\x9c\x1b%G",
        stride: 0,
        count: 0,
    },
    // **A UTF-8 character inside a `CSI`, one atom a `CSI` state**: it
    // cancels the sequence and is the ground's after a control, so the byte
    // after it is text. `fast_csi` declines at the lead in one read and the
    // graph walks it in every other, so the two paths meet here; a split
    // can fall inside the character, after the cancel.
    Atom {
        name: "csi-entry-lead",
        bytes: b"\x1b[\xe0\xa4\x83u",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "csi-param-lead",
        bytes: b"\x1b[5\xe0\xa4\x83C",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "csi-inter-lead",
        bytes: b"\x1b[ \xc3\xa9q",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "csi-ignore-cont",
        bytes: b"\x1b[1<\xa4m",
        stride: 0,
        count: 0,
    },
    // **A U+FFFD for ill-formed bytes after a Prepend is the character a
    // literal one is to the cluster rules**: a cell after U+0600 while
    // `?2027` is reset, joined to it by GB9b while set. A stray `80`, and a
    // lead cut short at the end of the atom, which the next atom's first
    // byte ends and which a split can fall inside.
    Atom {
        name: "fffd-after-prepend",
        bytes: b"a\xd8\x80\x80Z",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "fffd-cut-after-prepend",
        bytes: b"a\xd8\x80\xe2\x82",
        stride: 0,
        count: 0,
    },
    // **`OSC 52` writes, whose body leaves `pending` at `52;`** for the
    // clipboard areas and is decoded as it arrives: a split can fall inside
    // `52;` itself, where the route is decided, and between any two
    // characters of a quantum, where only the decoder's carry crosses. A
    // padded write closed by `ST`, an unpadded one by `BEL`, a `Pd` that is
    // not base64 (a clear), one cancelled by `CAN` mid-quantum, and a read.
    Atom {
        name: "osc52-write",
        bytes: b"\x1b]52;c;aGVsbG8gd29ybGQ=\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc52-unpadded",
        bytes: b"\x1b]52;p;Zm9vYg\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc52-clear",
        bytes: b"\x1b]52;;not base64\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc52-cancelled",
        bytes: b"\x1b]52;c;Zm9vY\x18",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc52-read",
        bytes: b"\x1b]52;c;?\x07",
        stride: 0,
        count: 0,
    },
    // **The VT420 rectangle operations, DECBI and DECFI**: each rewrites
    // cells on rows the cursor may be on, so a read that begins after one
    // must find the cursor's row as the operation left it, wide halves
    // included.
    Atom {
        name: "deccra",
        bytes: b"\x1b[1;1;2;6;1;3;4;1$v",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decfra",
        bytes: b"\x1b[42;2;2;3;5$x",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decera",
        bytes: b"\x1b[1;3;2;8$z",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsera",
        bytes: b"\x1b[2;1;3;6${",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "deccara",
        bytes: b"\x1b[1;1;4;9;1;4$r",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decrara",
        bytes: b"\x1b[1;2;3;7;7$t",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsace-rectangle",
        bytes: b"\x1b[2*x",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsace-stream",
        bytes: b"\x1b[0*x",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decrqcra",
        bytes: b"\x1b[1;1;1;1;3;5*y",
        stride: 0,
        count: 0,
    },
    // **REP after whatever wrote last**: each writer above records the
    // preceding graphic character its own way, the bulk ones once a run,
    // and a split read sends a run's characters through the per-character
    // writer instead.
    Atom {
        name: "rep",
        bytes: b"\x1b[2b",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decbi",
        bytes: b"\x1b6",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decfi",
        bytes: b"\x1b9",
        stride: 0,
        count: 0,
    },
    // **The line renditions**: a line of half the columns, which the fit
    // test, the wrap, the pin and every move read through the fence, and
    // whose size rides in the row's mark through every scroll. DECDWL and
    // DECDHL empty the half past the line's end and pull the cursor in.
    Atom {
        name: "decdwl",
        bytes: b"\x1b#6",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decdhl-top",
        bytes: b"\x1b#3",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decdhl-bottom",
        bytes: b"\x1b#4",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decswl",
        bytes: b"\x1b#5",
        stride: 0,
        count: 0,
    },
    // **What brings a line under the cursor in place**: `RI` and `SD` move
    // the rows with the cursor's row fixed, and `?47` swaps the surface, so
    // a sized line can arrive under a cursor that did not move onto it --
    // the direction a fence must never lag.
    Atom {
        name: "ri",
        bytes: b"\x1bM",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sd",
        bytes: b"\x1b[T",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "alt47-enter",
        bytes: b"\x1b[?47h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "alt47-leave",
        bytes: b"\x1b[?47l",
        stride: 0,
        count: 0,
    },
    // **Code extension**: the single shifts, which leave the parser in a node
    // of their own, the locking shifts into GL and GR, a 96-character set and
    // a line-drawing one in the shifted slots, and ISO 8859-1, so a GR byte and
    // the 8-bit single shifts are reached; a read split falls between a shift
    // and its character as often as anywhere.
    Atom {
        name: "ss2",
        bytes: b"\x1bN",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ss3",
        bytes: b"\x1bO",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ls2",
        bytes: b"\x1bn",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ls3",
        bytes: b"\x1bo",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ls1r",
        bytes: b"\x1b~",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ls2r",
        bytes: b"\x1b}",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ls3r",
        bytes: b"\x1b|",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "g2-latin1-96",
        bytes: b"\x1b.A",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "g3-graphics",
        bytes: b"\x1b+0",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "g1-greek-96",
        bytes: b"\x1b-F",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "latin1",
        bytes: b"\x1b%@",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "utf8",
        bytes: b"\x1b%G",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "gr-bytes",
        bytes: b"\xc1\xd7\xa0\xff",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ss2-8bit",
        bytes: b"\x8eq",
        stride: 0,
        count: 0,
    },
    // **The presentation state reports and their restore.**
    Atom {
        name: "deccir",
        bytes: b"\x1b[1$w",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "dectabsr",
        bytes: b"\x1b[2$w",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decrsps-cir",
        bytes: b"\x1bP1$t3;4;1;C;@;B;1;3;D;B0A<\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decrsps-tabs",
        bytes: b"\x1bP2$t5/9/40\x1b\\",
        stride: 0,
        count: 0,
    },
    // **ReGIS** (`DCS p`), last so no atom above moves: its interpreter
    // outlives the envelope, so an envelope cut mid-command leaves the next
    // one resuming inside it, and the reports and the counts must not
    // depend on where the reads fell.
    Atom {
        name: "regis-draw",
        bytes: b"\x1bPpS(E)W(I3)P[10,10]V[100,50]C[+20]F(V[50,50][60,60][40,60])T'Hi'\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "regis-macro-report",
        bytes: b"\x1bPp@:AV[+5,+5]S(T1)@;@A@AR(P)R(M(=))\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "regis-cut",
        bytes: b"\x1bPpP[1,2]T'a\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "regis-resync-display",
        bytes: b"\x1bP3pP[1.5e1,2]R(P)\nL(A1)\"a\"FF;\x1b\\",
        stride: 0,
        count: 0,
    },
    // **The status line** (DECSSDT, DECSASD), last so no atom above moves:
    // a read that ends on the line swaps the main display back, and the next
    // swaps the line in again, so what either holds must not depend on where
    // the reads fell. The write atom is text, a column move, a line feed
    // that erases the line and an erase, on whichever display is selected.
    Atom {
        name: "status-host-on",
        bytes: b"\x1b[2$~\x1b[1$}",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "status-write",
        bytes: b"ab\x1b[1mc\x1b[5Gd\ne\x1b[2Gf\x1b[K",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "status-main",
        bytes: b"\x1b[0$}",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "status-indicator",
        bytes: b"\x1b[1$~",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "status-none",
        bytes: b"\x1b[0$~",
        stride: 0,
        count: 0,
    },
    // **SCP and SPD**, last so no atom above moves: a direction selected
    // on the line the cursor is on and carried by the line rules -- a line
    // feed, a wrap, the lines a scroll or an erase brings in -- must not
    // depend on where the reads fell. Right to left, back to left to right
    // with the cursor sent to the line's start, and the whole screen.
    Atom {
        name: "scp-rtl",
        bytes: b"\x1b[2 k",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "scp-ltr-home",
        bytes: b"\x1b[1;1 k",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "spd-rtl",
        bytes: b"\x1b[3 S",
        stride: 0,
        count: 0,
    },
    // **terminal-wg's `?2500` and `?2501`**, last for the same reason: a
    // paragraph property set at a paragraph's start, or in force for the
    // lines that follow, must not depend on where the reads fell either.
    Atom {
        name: "box-mirror-on",
        bytes: b"\x1b[?2500h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "autodetect-off",
        bytes: b"\x1b[?2501l",
        stride: 0,
        count: 0,
    },
    // **SAPV**, last for the same reason: a form waits for the following
    // graphic character, which may arrive in a later read, as one at a
    // time, in a run of ASCII or of two-byte letters; held forms, digits
    // and the decimal mark ride the rendition; and 0 cancels them all.
    Atom {
        name: "sapv-form",
        bytes: b"\x1b[6 ]",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sapv-held",
        bytes: b"\x1b[2;10;21;7 ]",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sapv-default",
        bytes: b"\x1b[ ]",
        stride: 0,
        count: 0,
    },
    // **An alternative font**, last: its number shares a word of the
    // rendition table's hash with SAPV's variants, so the two must part
    // every rendition they reach together -- font 1 alone, and font 1 with
    // the variants' first bit, SAPV 2, set by the same atom.
    Atom {
        name: "sgr-font",
        bytes: b"\x1b[11m",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sgr-font-digits",
        bytes: b"\x1b[11m\x1b[2 ]",
        stride: 0,
        count: 0,
    },
    // **SDS and SRS**, last: a string begun rides the rendition being
    // built, an end puts back the one around it, and an erase keeps each
    // position's string, none of which may depend on where the reads fell;
    // a string beside the font shares its byte of the rendition word.
    Atom {
        name: "sds-rtl",
        bytes: b"\x1b[2]",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "srs-begin",
        bytes: b"\x1b[1[",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sds-end",
        bytes: b"\x1b[0]",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "srs-end",
        bytes: b"\x1b[0[",
        stride: 0,
        count: 0,
    },
    // An underline with a string begun: the two renditions a key laying
    // the string over `attrs` gave one key.
    Atom {
        name: "sgr-underline-sds",
        bytes: b"\x1b[4m\x1b[2]",
        stride: 0,
        count: 0,
    },
    // **DECRLM**, last: while set every character goes one at a time to
    // the leftward writer, a wrap waits at the left margin, and a mark joins
    // the cell right of the cursor; none of it may depend on where the
    // reads fell, nor on whether a run or its atoms one by one were read.
    Atom {
        name: "decrlm-on",
        bytes: b"\x1b[?34h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decrlm-off",
        bytes: b"\x1b[?34l",
        stride: 0,
        count: 0,
    }, // **BDSM reset and DCSM**, last: with BDSM reset and DCSM at
    // PRESENTATION, the cursor functions, `CR`, a line feed and the editing
    // functions take the cursor's position as drawn on a line holding a
    // string, and `CUF` and `CUB` run as drawn on a right-to-left line;
    // none of it may depend on where the reads fell. `HVP` and `HPA` move
    // by cell in every state.
    Atom {
        name: "bdsm-explicit",
        bytes: b"\x1b[8l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "dcsm-data",
        bytes: b"\x1b[9h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "dcsm-presentation",
        bytes: b"\x1b[9l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "hvp",
        bytes: b"\x1b[1;3f",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "hpa",
        bytes: b"\x1b[2`",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cuf",
        bytes: b"\x1b[2C",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cub",
        bytes: b"\x1b[D",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cud",
        bytes: b"\x1b[B",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cha",
        bytes: b"\x1b[3G",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ht",
        bytes: b"\t",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cbt",
        bytes: b"\x1b[Z",
        stride: 0,
        count: 0,
    },
    // **HEM, IRM and the editing functions**, last: under PRECEDING `DCH`
    // and `ICH` shift the cells before the cursor and a character written
    // under IRM goes in at the cursor, which stays; under DCSM's
    // PRESENTATION each shifts the positions as drawn, and `EA` and `EF`
    // erase them; `CNL` goes to the cell drawn at the line's first
    // position. None of it may depend on where the reads fell.
    Atom {
        name: "hem-preceding",
        bytes: b"\x1b[10h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "hem-following",
        bytes: b"\x1b[10l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "irm-on",
        bytes: b"\x1b[4h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "irm-off",
        bytes: b"\x1b[4l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "dch",
        bytes: b"\x1b[2P",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ich",
        bytes: b"\x1b[@",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ea",
        bytes: b"\x1b[O",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ef",
        bytes: b"\x1b[1N",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cnl",
        bytes: b"\x1b[E",
        stride: 0,
        count: 0,
    },
    // **TSR and HTJ**, at the end so the seeds saved before them keep their
    // atoms: the stop at column 9 removed, a field justified against the
    // next stop, and HTJ's C1 form, which is HTJ under ISO 8859-1 alone.
    Atom {
        name: "tsr",
        bytes: b"\x1b[9 d",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "htj",
        bytes: b"\x1bI",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "htj-8bit",
        bytes: b"\x89",
        stride: 0,
        count: 0,
    },
    // **SLH and SLL**: a line home at 5 and a line limit at 12, which send
    // the text a character at a time and wrap it at the limit to the home.
    Atom {
        name: "slh",
        bytes: b"\x1b[5 U",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "sll",
        bytes: b"\x1b[12 V",
        stride: 0,
        count: 0,
    },
    // **SEE**: the page, whose shifts cross line ends, and the line again.
    Atom {
        name: "see-page",
        bytes: b"\x1b[Q",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "see-line",
        bytes: b"\x1b[1Q",
        stride: 0,
        count: 0,
    },
    // **Aligned stops**: TALE at 9 and TCC at 17 on `.`, which an `HT` then
    // reaches, and the `ht` atom above with them.
    Atom {
        name: "tale",
        bytes: b"\x1b[9 a",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "tcc",
        bytes: b"\x1b[17;46 c",
        stride: 0,
        count: 0,
    },
    // **GATM**, at the end so the seeds saved before it keep their atoms:
    // ALL, a print taking the guarded positions too, and GUARD again.
    Atom {
        name: "gatm-all",
        bytes: b"\x1b[1h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "gatm-guard",
        bytes: b"\x1b[1l",
        stride: 0,
        count: 0,
    },
    // **NP and PPA** on the one page: NP homes the cursor, and PPA 2 asks
    // for a page baud lacks and keeps the line and column.
    Atom {
        name: "np",
        bytes: b"\x1b[2U",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ppa",
        bytes: b"\x1b[2 P",
        stride: 0,
        count: 0,
    },
    // **A second line limit, HPR and CHT**, at the end so the seeds saved
    // before them keep their atoms: an SLL at 5 moves `sll`'s limit, and a
    // wrap waiting at either, and HPR and CHT move from such a wait.
    Atom {
        name: "sll-near",
        bytes: b"\x1b[5 V",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "hpr",
        bytes: b"\x1b[a",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "cht",
        bytes: b"\x1b[I",
        stride: 0,
        count: 0,
    },
    // **HTS and TBC 0**, at the end for the seeds: a stop set and cleared
    // where a wrap waits, at the last column, a margin or a limit.
    Atom {
        name: "hts",
        bytes: b"\x1bH",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "tbc",
        bytes: b"\x1b[g",
        stride: 0,
        count: 0,
    },
    // The shell's prompt marks, last so the seeds before them keep their
    // meaning: an initial mark in an open prompt zone is a redraw or the
    // prompt drawn again by where the cursor is, never by where a read
    // ended.
    Atom {
        name: "osc133_a",
        bytes: b"\x1b]133;A\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc133_b",
        bytes: b"\x1b]133;B\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc133_p",
        bytes: b"\x1b]133;P;k=i\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc133_c",
        bytes: b"\x1b]133;C\x07",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc133_d",
        bytes: b"\x1b]133;D;0\x07",
        stride: 0,
        count: 0,
    },
    // `OSC 8`, opened under two targets and closed, and the operations that
    // carry a linked cell to another row or write one without the text
    // writer: what the link table's sweep must still read.
    Atom {
        name: "osc8-open",
        bytes: b"\x1b]8;;http://p.example/a\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc8-open-id",
        bytes: b"\x1b]8;id=k;http://p.example/b\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "osc8-close",
        bytes: b"\x1b]8;;\x1b\\",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "il",
        bytes: b"\x1b[2L",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "dl",
        bytes: b"\x1b[2M",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decaln",
        bytes: b"\x1b#8",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decstbm-decslrm",
        bytes: b"\x1b[3;12r\x1b[?69h\x1b[4;15s",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "su",
        bytes: b"\x1b[3S",
        stride: 0,
        count: 0,
    },
    // HP's memory lock and unlock: the top margin to the cursor's line
    // and back to the first, the cursor left where it is.
    Atom {
        name: "memory-lock",
        bytes: b"\x1bl",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "memory-unlock",
        bytes: b"\x1bm",
        stride: 0,
        count: 0,
    },
    // A selected area around two letters, and the three modes that choose
    // what a print takes of it, set and reset.
    Atom {
        name: "ssa-esa",
        bytes: b"\x1bFab\x1bG",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "area-modes-set",
        bytes: b"\x1b[15;16;17h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "area-modes-reset",
        bytes: b"\x1b[15;16;17l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "alt1049-enter",
        bytes: b"\x1b[?1049h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "alt1049-leave",
        bytes: b"\x1b[?1049l",
        stride: 0,
        count: 0,
    },
    // **DECLRMM, DECSLRM and the displays apart**, so margin mode set or
    // reset on either display, margins set on either and the line entered
    // and left between them are generated: `decstbm-decslrm` sets the mode
    // and the margins together and only ever on, so no program reset the
    // mode on one display with margins set on the other.
    Atom {
        name: "declrmm-on",
        bytes: b"\x1b[?69h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "declrmm-off",
        bytes: b"\x1b[?69l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decslrm",
        bytes: b"\x1b[5;16s",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decssdt-host",
        bytes: b"\x1b[2$~",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsasd-status",
        bytes: b"\x1b[1$}",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decstr",
        bytes: b"\x1b[!p",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "deccolm",
        bytes: b"\x1b[?40h\x1b[?3l",
        stride: 0,
        count: 0,
    },
    // **Grapheme cluster mode, and the text it decides the cells of.**
    // `?2027` set and reset, and Devanagari, where a cell holds an extended
    // cluster while it is set and a legacy one while reset: a word with a
    // reph, a virama, a conjunct and a pre-base vowel sign (टर्मिनल), a
    // conjunct alone (क्ष), a vowel sign alone, which parts from or joins
    // whatever atom came before, a visarga (दुःख), and the Thai SARA AM
    // (กำ). Each is a run of one encoded length, so the run scanner sees
    // it, and a read can cut any of them.
    Atom {
        name: "grapheme-mode-set",
        bytes: b"\x1b[?2027h",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "grapheme-mode-reset",
        bytes: b"\x1b[?2027l",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "devanagari-word",
        bytes:
            b"\xe0\xa4\x9f\xe0\xa4\xb0\xe0\xa5\x8d\xe0\xa4\xae\xe0\xa4\xbf\xe0\xa4\xa8\xe0\xa4\xb2",
        stride: 3,
        count: 7,
    },
    Atom {
        name: "devanagari-conjunct",
        bytes: b"\xe0\xa4\x95\xe0\xa5\x8d\xe0\xa4\xb7",
        stride: 3,
        count: 3,
    },
    Atom {
        name: "devanagari-vowel-sign",
        bytes: b"\xe0\xa4\xbf",
        stride: 3,
        count: 1,
    },
    Atom {
        name: "devanagari-visarga",
        bytes: b"\xe0\xa4\xa6\xe0\xa5\x81\xe0\xa4\x83\xe0\xa4\x96",
        stride: 3,
        count: 4,
    },
    Atom {
        name: "thai-sara-am",
        bytes: b"\xe0\xb8\x81\xe0\xb8\xb3",
        stride: 3,
        count: 2,
    },
    // **The erases, each a store path of its own** (`View::fill_raw`, the
    // history's `erase_outside` and `selective_wipe_behind`, the full
    // reset's `erase_ring` and the other surface's): what
    // `writes_move_the_rows_they_touch` needs the generator to reach.
    Atom {
        name: "ed",
        bytes: b"\x1b[J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ed1",
        bytes: b"\x1b[1J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ed2",
        bytes: b"\x1b[2J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ech",
        bytes: b"\x1b[3X",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsed",
        bytes: b"\x1b[?J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ed3",
        bytes: b"\x1b[3J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "decsed3",
        bytes: b"\x1b[?3J",
        stride: 0,
        count: 0,
    },
    Atom {
        name: "ris",
        bytes: b"\x1bc",
        stride: 0,
        count: 0,
    },
];

/// Either ambiguous-width policy.
fn policy() -> impl Strategy<Value = AmbiguousWidth> {
    prop_oneof![Just(AmbiguousWidth::Narrow), Just(AmbiguousWidth::Wide)]
}

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
/// title, the reply, and the clipboard -- how many writes were published and
/// what the last left, `None` in its octets for a clear.
type Seen = (
    Vec<PackedCell>,
    bool,
    Vec<u8>,
    Vec<u8>,
    Clipped,
    Vec<(LineRendition, baud_domain::LineBidi)>,
    baud_domain::RegisCounts,
    Status,
);

/// The status line as a driver reads it: its type, the display selected,
/// its cells and its cursor.
type Status = (
    StatusType,
    ActiveDisplay,
    Option<(Vec<PackedCell>, Option<Col>)>,
);

fn status(t: &Term<'_>) -> Status {
    (
        t.status_type(),
        t.active_display(),
        t.status_line().map(|l| (l.cells.to_vec(), l.cursor)),
    )
}

/// Every row's line rendition and paragraph properties, which the cells
/// do not show -- and the promise the rendition's bits ride on: a sized row's mark
/// is at least the width, so `RowExtent::reached`'s bare `max` never has to
/// raise it.
fn renditions(t: &Term<'_>) -> Vec<(LineRendition, baud_domain::LineBidi)> {
    (0..ROWS)
        .map(|y| {
            let r = t.line_rendition(Row::from_index(y)).expect("a row");
            if r.is_double() {
                let mark = t.row_extent(Row::from_index(y)).index();
                assert!(mark >= COLS, "row {y} is {r:?} under a mark of {mark}");
            }
            (r, t.row_extent(Row::from_index(y)).bidi())
        })
        .collect()
}

type Clipped = (ClipboardWrites, Option<(Selections, Option<Vec<u8>>)>);

/// Room for every write the alphabet makes.
const CLIP: ClipboardBudget = ClipboardBudget::of_text(64);

fn clipped(t: &Term<'_>) -> Clipped {
    let last = t.clipboard().map(|w| {
        let bytes = match w.content {
            ClipboardContent::Set(b) => Some(b.to_vec()),
            ClipboardContent::Cleared => None,
        };
        (w.targets, bytes)
    });
    (t.clipboard_writes(), last)
}

/// Feed `bytes` in `chunk`-sized reads and return **every cell**, not a row
/// and not a string projection -- whether the cursor is shown, the one mode
/// the alphabet sets, saves and restores, the title, the one string it sets,
/// and the reply, to the one request it makes.
fn drive(bytes: &[u8], chunk: usize) -> Seen {
    drive_at(bytes, chunk, AmbiguousWidth::Narrow)
}

/// [`drive`] under ambiguous-width policy `a`: box drawing, East Asian
/// Ambiguous, is one column under `Narrow` and two under `Wide`, so the two
/// put the run writers' width decisions in different places.
fn drive_at(bytes: &[u8], chunk: usize, a: AmbiguousWidth) -> Seen {
    let w = ws();
    let mut owned = Owned::new(&w).with_clipboard(CLIP);
    let mut t = Term::new(owned.storage(), &w);
    t.set_ambiguous_width(a);
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
        clipped(&t),
        renditions(&t),
        t.regis_counts(),
        status(&t),
    )
}

/// The same program, but each **atom** delivered in its own `feed`.
///
/// **A different mechanism, not a different chunk size.** Splitting by byte is
/// chunk invariance again; splitting by atom makes every atom its own run, so
/// a run of four and four runs of one must agree. That is the class chunk
/// invariance is structurally blind to -- where both chunkings are wrong the
/// same way, which is what the blitter's clipping bug was.
fn drive_atomwise(prog: &[usize], a: AmbiguousWidth) -> Seen {
    let w = ws();
    let mut owned = Owned::new(&w).with_clipboard(CLIP);
    let mut t = Term::new(owned.storage(), &w);
    t.set_ambiguous_width(a);
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
        clipped(&t),
        renditions(&t),
        t.regis_counts(),
        status(&t),
    )
}

/// **The control, and nothing below it is believable until it passes.**
///
/// An atom that silently emits the wrong bytes is indistinguishable from a
/// shape that is never buggy -- the same failure as a search probe that cannot
/// match, and the reason `baud-core2`'s suite ran a positive control over its
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
    let (_, _, title, _, _, _, _, _) = drive(&bytes_of(&[atom("osc-c1-utf8")]), 4096);
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

/// **The DCS atom with an `LF` and a `DEL` reaches the handler it was added
/// for**: the request answers valid, so neither byte is in the `Pt`, and the
/// cursor has not moved, so the `LF` was not executed. A `DEL` the graph
/// stored would answer invalid in both chunkings and every property would
/// stay green.
#[test]
fn the_dcs_lf_del_atom_reaches_the_handler() {
    fed("dcs-lf-del", &|t| {
        assert_eq!(t.reply(), b"\x1bP1$r0m\x1b\\", "DCS: LF m DEL names SGR");
        assert_eq!(
            t.cursor(),
            (Row::ZERO, Col::ZERO),
            "DCS: the LF ran nothing"
        );
    });
}

/// **The GR prefix atom reaches DECRQSS**: the request answers, so `B1 BB B2
/// A4 F1` was read as `1;2$q`, and nothing was drawn. A prefix that sent its
/// first GR byte to the body would leave the string ignored and unanswered in
/// both chunkings, and every property would stay green.
#[test]
fn the_latin1_dcs_gr_atom_reaches_decrqss() {
    fed("latin1-dcs-gr", &|t| {
        assert_eq!(t.reply(), b"\x1bP1$r0m\x1b\\", "DCS: GR $ q names SGR");
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "DCS: nothing drawn");
        assert_eq!(t.strings_ignored, 0, "DCS: the string was answered");
    });
}

/// **The rectangle atoms reach their operations**, the control the C1
/// atoms have: an atom the grammar counted and dropped would change nothing
/// in either chunking, and every property would stay green.
#[test]
fn the_rectangle_atoms_reach_their_operations() {
    fed("decfra", &|t| {
        let row = t.row(Row::from_index(1)).expect("row 2");
        assert_eq!(
            row[1].content.as_codepoint().map(|c| c.value()),
            Some(0x2a),
            "DECFRA filled"
        );
        assert_eq!(t.unsupported, 0);
    });
    fed("decrqcra", &|t| {
        assert!(t.reply().starts_with(b"\x1bP1!~"), "DECRQCRA answered")
    });
    fed("decfi", &|t| {
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(1)), "DECFI moved")
    });
    for name in [
        "deccra",
        "decera",
        "decsera",
        "deccara",
        "decrara",
        "decsace-rectangle",
        "decbi",
    ] {
        fed(name, &|t| {
            assert_eq!(t.unsupported, 0, "{name} is acted on")
        });
    }
}

/// **The intermediate atom designates and draws what follows**, in one read
/// and in one-byte reads: an `ESC ( ]` or `ESC ( % P` that opened a string
/// would swallow the `u` and the `v`. One read alone cannot tell, because
/// `fast_esc` takes `ESC ( ]` whole; the two intermediates of `ESC ( % P` it
/// declines, so there the graph decides in every chunking, and only this
/// control sees it.
#[test]
fn the_esc_inter_atom_draws_what_follows() {
    for chunk in [1, 4096] {
        let (cells, ..) = drive(&bytes_of(&[atom("esc-inter-introducer")]), chunk);
        let drawn: Vec<u32> = cells[..3]
            .iter()
            .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
            .collect();
        assert_eq!(
            drawn,
            [u32::from(b'u'), u32::from(b'v'), 0],
            "reads of {chunk}: u and v drawn"
        );
    }
}

/// **The UTF-8 atoms inside a `CSI` and after a Prepend reach the edges they
/// were added for**, the control the C1 atoms have: a character the `CSI`
/// swallowed would do so in every chunking. After U+0600, a column of its
/// own, the U+FFFD takes the next cell while `?2027` is reset and joins the
/// sign while it is set, as a literal U+FFFD does.
#[test]
fn the_csi_and_prepend_atoms_reach_their_edges() {
    let row = |t: &Term<'_>| -> Vec<u32> {
        t.viewport()[..2]
            .iter()
            .map(|c| t.base_of(*c).as_codepoint().map_or(0, |c| c.value()))
            .collect()
    };
    for (name, want) in [
        ("csi-entry-lead", [0x903, u32::from(b'u')]),
        ("csi-param-lead", [0x903, u32::from(b'C')]),
        ("csi-inter-lead", [0xe9, u32::from(b'q')]),
        ("csi-ignore-cont", [0xfffd, u32::from(b'm')]),
    ] {
        fed(name, &|t| {
            assert_eq!(row(t), want, "{name}: cancelled and printed");
            assert_eq!(t.cursor().1, Col::at_index(2), "{name}: the cursor");
        });
    }
    let cells = |t: &Term<'_>, n: usize| -> Vec<u32> {
        t.viewport()[..n]
            .iter()
            .map(|c| t.base_of(*c).as_codepoint().map_or(0, |c| c.value()))
            .collect()
    };
    fed("fffd-after-prepend", &|t| {
        assert_eq!(
            cells(t, 4),
            [u32::from(b'a'), 0x600, 0xfffd, u32::from(b'Z')]
        );
    });
    fed_after(b"\x1b[?2027h", "fffd-after-prepend", &|t| {
        assert_eq!(cells(t, 3), [u32::from(b'a'), 0x600, u32::from(b'Z')]);
        let marks: Vec<u32> = t.marks(Row::ZERO, Col::at_index(1)).collect();
        assert_eq!(marks, [0xfffd], "the U+FFFD joins the Prepend");
    });
}

/// **The grapheme cluster mode atoms reach both cells**: टर्मिनल fed alone
/// takes six columns, a legacy cluster a cell, and after the set atom four,
/// ट, र्मि, न and ल; the reset atom returns to six.
#[test]
fn the_grapheme_mode_atoms_reach_both_cells() {
    fed("devanagari-word", &|t| {
        assert_eq!(t.cursor().1, Col::at_index(6))
    });
    let set = bytes_of(&[atom("grapheme-mode-set")]);
    fed_after(&set, "devanagari-word", &|t| {
        assert_eq!(t.cursor().1, Col::at_index(4))
    });
    let both = bytes_of(&[atom("grapheme-mode-set"), atom("grapheme-mode-reset")]);
    fed_after(&both, "devanagari-word", &|t| {
        assert_eq!(t.cursor().1, Col::at_index(6))
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
    fed_after(b"", name, f);
}

/// [`fed`] once `setup` has been fed in a read of its own.
fn fed_after(setup: &[u8], name: &str, f: &dyn Fn(&Term<'_>)) {
    let bytes = bytes_of(&[atom(name)]);
    let w = ws();
    let mut owned = Owned::new(&w).with_clipboard(CLIP);
    let mut t = Term::new(owned.storage(), &w);
    if !setup.is_empty() {
        t.write_at(BytePos::ZERO, setup);
        t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
    }
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    f(&t);
}

/// **The memory lock atoms move the top margin**: `memory-lock` to the
/// cursor's line, `memory-unlock` back to the first, the bottom margin and
/// the cursor kept by both.
#[test]
fn the_memory_lock_atoms_move_the_top_margin() {
    let last = Row::from_index(ROWS - 1);
    fed_after(b"\x1b[4;3H", "memory-lock", &|t| {
        assert_eq!(t.scroll_region(), (Row::from_index(3), last));
        assert_eq!(t.cursor(), (Row::from_index(3), Col::at_index(2)));
    });
    fed_after(b"\x1b[4;9r\x1b[6;1H", "memory-unlock", &|t| {
        assert_eq!(t.scroll_region(), (Row::ZERO, Row::from_index(8)));
        assert_eq!(t.cursor(), (Row::from_index(5), Col::ZERO));
    });
}

/// **The selected area atoms reach SSA, ESA and the three modes**: none
/// is counted, the area's letters are written where they go, and DECRQM
/// answers each mode set, then reset.
#[test]
fn the_selected_area_atoms_reach_ssa_esa_and_the_modes() {
    fed("ssa-esa", &|t| {
        assert_eq!(t.unsupported, 0);
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(2)));
    });
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    for (atom_name, want) in [("area-modes-set", 1), ("area-modes-reset", 2)] {
        support::feed(&mut t, &bytes_of(&[atom(atom_name)]));
        for m in [15, 16, 17] {
            t.clear_reply();
            support::feed(&mut t, format!("\x1b[{m}$p").as_bytes());
            assert_eq!(t.reply(), format!("\x1b[{m};{want}$y").as_bytes());
        }
    }
    assert_eq!(t.unsupported_param, 0);
}

/// **The ReGIS atoms reach ReGIS**, the control the guard atoms have:
/// each enters an envelope, the macrograph atom's report answers the
/// position its two plays moved to and the store's room, and the cut
/// atom leaves a quoted string open for the next envelope to resume in.
#[test]
fn the_regis_atoms_reach_regis() {
    fed("regis-macro-report", &|t| {
        assert_eq!(t.regis_counts().entered, 1);
        assert_eq!(t.regis_counts().delays_unheld, 2, "one a play");
        assert_eq!(t.reply(), b"[10,10]\r\"9987,10000\"\r");
    });
    fed("regis-draw", &|t| assert_eq!(t.regis_counts().entered, 1));
    let bytes = bytes_of(&[atom("regis-cut"), atom("regis-macro-report")]);
    let w = ws();
    let mut owned = Owned::new(&w).with_clipboard(CLIP);
    let mut t = Term::new(owned.storage(), &w);
    support::feed(&mut t, &bytes);
    assert_eq!(t.reply(), b"", "the second envelope is inside the quote");
}

/// **The status atoms reach the line**, the control the guard atoms have:
/// the write atom lands on the host-writable line after `status-host-on`,
/// on the main display after `status-main`, and the indicator shows.
#[test]
fn the_status_atoms_reach_the_line() {
    let bytes = bytes_of(&[atom("status-host-on"), atom("status-write")]);
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    support::feed(&mut t, &bytes);
    let line = t.status_line().expect("the host-writable line");
    let drawn: Vec<u32> = line
        .cells
        .iter()
        .filter_map(|c| c.content.as_codepoint().map(|cp| cp.value()))
        .collect();
    // The line feed erased `abcd`, and the erase after `f` took `e`.
    assert_eq!(drawn, [u32::from(b'f')]);
    support::feed(
        &mut t,
        &bytes_of(&[atom("status-main"), atom("status-indicator")]),
    );
    assert_eq!(t.active_display(), ActiveDisplay::Main);
    assert_eq!(t.status_type(), StatusType::Indicator);
    support::feed(&mut t, &bytes_of(&[atom("status-none")]));
    assert!(t.status_line().is_none());
}

/// **The margin-mode atoms reach DECLRMM, DECSLRM and both displays**,
/// the control the status atoms have: `decslrm` sets margins only under
/// `declrmm-on` and `declrmm-off` puts them back, on the main display and
/// on the line, each answered by DECRQM and DECRQSS on the display that
/// received them; `decssdt-host` and `decsasd-status` select the line, and
/// `decstr` leaves it; `deccolm` clears the screen. An atom that never
/// reached its control would leave the margin mode where it was, and no
/// program would reset it with margins set on the other display.
#[test]
fn the_margin_mode_atoms_reach_both_displays() {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let ask = |t: &mut Term<'_>| {
        t.clear_reply();
        support::feed(t, b"\x1b[?69$p\x1bP$qs\x1b\\");
        t.reply().to_vec()
    };
    support::feed(&mut t, &bytes_of(&[atom("decslrm")]));
    assert_eq!(
        ask(&mut t),
        b"\x1b[?69;2$y\x1bP1$r1;20s\x1b\\",
        "SCOSC under ?69l"
    );
    support::feed(&mut t, &bytes_of(&[atom("declrmm-on"), atom("decslrm")]));
    assert_eq!(ask(&mut t), b"\x1b[?69;1$y\x1bP1$r5;16s\x1b\\");
    support::feed(&mut t, &bytes_of(&[atom("declrmm-off")]));
    assert_eq!(ask(&mut t), b"\x1b[?69;2$y\x1bP1$r1;20s\x1b\\");
    support::feed(&mut t, &bytes_of(&[atom("decssdt-host")]));
    assert_eq!(t.status_type(), StatusType::HostWritable);
    assert_eq!(t.active_display(), ActiveDisplay::Main);
    support::feed(&mut t, &bytes_of(&[atom("decsasd-status")]));
    assert_eq!(t.active_display(), ActiveDisplay::Status);
    support::feed(&mut t, &bytes_of(&[atom("declrmm-on"), atom("decslrm")]));
    assert_eq!(
        ask(&mut t),
        b"\x1b[?69;1$y\x1bP1$r5;16s\x1b\\",
        "the line's margins"
    );
    support::feed(&mut t, &bytes_of(&[atom("declrmm-off")]));
    assert_eq!(
        ask(&mut t),
        b"\x1b[?69;2$y\x1bP1$r1;20s\x1b\\",
        "the line's margins"
    );
    support::feed(&mut t, &bytes_of(&[atom("decstr")]));
    assert_eq!(t.active_display(), ActiveDisplay::Main);
    support::feed(&mut t, b"x");
    assert!(
        t.row(Row::ZERO)
            .is_some_and(|r| r[0].content != Content::EMPTY)
    );
    support::feed(&mut t, &bytes_of(&[atom("deccolm")]));
    assert!(
        t.row(Row::ZERO)
            .is_some_and(|r| r[0].content == Content::EMPTY)
    );
}

/// **The path atoms reach the rows**, the control the status atoms have:
/// `scp-rtl` turns the cursor's line and a line feed carries it, a line the
/// cursor moves to stays as it was until `scp-ltr-home` turns it and sends
/// the cursor to its start, and `spd-rtl` turns every line. An atom that
/// never reached SCP would leave every row without a path, and every
/// property would compare rows that hold none.
#[test]
fn the_path_atoms_reach_the_rows() {
    use baud_domain::CharacterPath::{Default as None_, LeftToRight, RightToLeft};
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let path = |t: &Term<'_>, y: usize| t.row_extent(Row::from_index(y)).path();
    support::feed(&mut t, &bytes_of(&[atom("scp-rtl")]));
    support::feed(&mut t, b"ab\r\n\x1b[4;3H");
    assert_eq!(
        (path(&t, 0), path(&t, 1), path(&t, 3)),
        (RightToLeft, RightToLeft, None_)
    );
    support::feed(&mut t, &bytes_of(&[atom("scp-ltr-home")]));
    assert_eq!(
        (path(&t, 3), t.cursor()),
        (LeftToRight, (Row::from_index(3), Col::ZERO))
    );
    support::feed(&mut t, &bytes_of(&[atom("spd-rtl")]));
    assert!((0..ROWS).all(|y| path(&t, y) == RightToLeft));
}

/// **The DCSM atoms reach the presentation**, the control the path atoms
/// have: with `bdsm-explicit`, `ab` reversed by `srs-begin` is drawn `ba`,
/// so `cup-home` lands on `b`, column 1, in both DCSM states, and `hvp`
/// and `hpa` take their columns in every state; `ich` on `b` opens its
/// blank after it under PRESENTATION and before it after `dcsm-data`; on
/// an `scp-rtl` line `cuf` steps right on the screen, toward column 0. An
/// atom that never reached DCSM would leave every editing function by
/// cell, and every property would compare cells no presentation moved.
#[test]
fn the_dcsm_atoms_reach_the_presentation() {
    let fed_then = |names: &[&str]| {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let prog: Vec<usize> = names.iter().map(|n| atom(n)).collect();
        support::feed(&mut t, &bytes_of(&prog));
        t.cursor().1
    };
    let strung = ["bdsm-explicit", "srs-begin", "ascii-short", "srs-end"];
    let with = |more: &[&str]| fed_then(&[&strung[..], more].concat());
    assert_eq!(with(&["cup-home"]), Col::at_index(1));
    assert_eq!(with(&["dcsm-data", "cup-home"]), Col::at_index(1));
    let cells = |names: &[&str]| {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let prog: Vec<usize> = [&strung[..], names]
            .concat()
            .iter()
            .map(|n| atom(n))
            .collect();
        support::feed(&mut t, &bytes_of(&prog));
        t.row(Row::ZERO).expect("a row")[..3]
            .iter()
            .map(|c| c.content.as_codepoint().map_or(0, |cp| cp.value()))
            .collect::<Vec<u32>>()
    };
    let (a, b) = (u32::from(b'a'), u32::from(b'b'));
    assert_eq!(cells(&["hpa", "ich"]), [a, b, 0]);
    assert_eq!(cells(&["dcsm-data", "hpa", "ich"]), [a, 0, b]);
    assert_eq!(
        cells(&["dcsm-data", "dcsm-presentation", "hpa", "ich"]),
        [a, b, 0]
    );
    assert_eq!(with(&["hvp"]), Col::at_index(2));
    assert_eq!(with(&["hpa"]), Col::at_index(1));
    assert_eq!(
        fed_then(&["bdsm-explicit", "scp-rtl", "ascii-short", "cuf"]),
        Col::ZERO
    );
    assert_eq!(
        fed_then(&["scp-rtl", "ascii-short", "cuf"]),
        Col::at_index(4)
    );
}

/// **The HEM atoms reach the editing functions**, the control the DCSM
/// atoms have: on `the quick brown fox`, `cup-home` then `cuf` puts the
/// cursor on `e`, column 2; `hem-preceding` and `dch` then remove `h` and
/// `e` and close up from the line's start, and `irm-on` writes `ab` at the
/// cursor, which stays, pushing `t` and `h` off; `hem-following` puts both
/// back to the cells after the cursor. `ea`, `ef` and `cnl` erase and move
/// as they always did on a line with no string. An atom that never reached
/// HEM would leave every shift on the cells after the cursor.
#[test]
fn the_hem_atoms_reach_the_editing_functions() {
    let fed_then = |names: &[&str]| {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let prog: Vec<usize> = ["ascii", "cup-home", "cuf"]
            .iter()
            .chain(names)
            .map(|n| atom(n))
            .collect();
        support::feed(&mut t, &bytes_of(&prog));
        let text: String = t.row(Row::ZERO).expect("a row")[..6]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        (text, t.cursor().1)
    };
    let col = Col::at_index;
    assert_eq!(
        fed_then(&["hem-preceding", "dch"]),
        ("..t qu".into(), col(2))
    );
    assert_eq!(
        fed_then(&["hem-following", "dch"]),
        ("thquic".into(), col(2))
    );
    assert_eq!(
        fed_then(&["hem-preceding", "irm-on", "ascii-short"]),
        ("eab qu".into(), col(2))
    );
    assert_eq!(
        fed_then(&["hem-preceding", "irm-on", "irm-off", "ascii-short"]),
        ("thabqu".into(), col(4))
    );
    assert_eq!(
        fed_then(&["hem-preceding", "hem-following", "irm-on", "ascii-short"]),
        ("thabe ".into(), col(4))
    );
    assert_eq!(
        fed_then(&["hem-preceding", "ich"]),
        ("he. qu".into(), col(2))
    );
    assert_eq!(fed_then(&["ef"]), ("... qu".into(), col(2)));
    assert_eq!(fed_then(&["ea"]), ("th....".into(), col(2)));
    assert_eq!(fed_then(&["cnl"]).1, Col::ZERO);
}

/// **The SAPV atoms reach the rendition**, the control the path atoms
/// have: `sapv-form` gives the next character a form and the one after it
/// none, `sapv-held` holds one for every character after it, and
/// `sapv-default` takes them all away. An atom that never reached SAPV
/// would leave every rendition at the default, and every property would
/// compare cells that carry no variant.
#[test]
fn the_sapv_atoms_reach_the_rendition() {
    use baud_domain::{Digits, Form};
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let at = |t: &Term<'_>, x: usize| {
        let cell = t.row(Row::ZERO).expect("a row")[x];
        t.styles().one().slots()[cell.style.index()]
            .word()
            .expect("a live rendition")
            .variants()
    };
    support::feed(&mut t, &bytes_of(&[atom("sapv-form")]));
    support::feed(&mut t, "\u{628}\u{628}".as_bytes());
    assert_eq!(
        (at(&t, 0).form(), at(&t, 1).form()),
        (Some(Form::Initial), None)
    );
    support::feed(&mut t, &bytes_of(&[atom("sapv-held")]));
    support::feed(&mut t, b"cd");
    assert_eq!(
        (at(&t, 2).form(), at(&t, 3).form()),
        (Some(Form::Medial), Some(Form::Medial))
    );
    assert_eq!(at(&t, 3).digits(), Digits::ArabicIndic);
    support::feed(&mut t, &bytes_of(&[atom("sapv-default")]));
    support::feed(&mut t, b"e");
    assert!(at(&t, 4).is_default());
}

/// **Two renditions SAPV alone parts are numbered the same at any read
/// size**: pinned from 20,000 cases. A form taken by a Latin-1 run, then
/// the status line's indicator, drawn at a read's end, and DECRARA,
/// interned the reversed renditions of a cell with the form and one
/// without in an order the reads decided; with the variants left out of
/// the rendition table's hash the two shared a bucket and swapped numbers.
#[test]
fn renditions_sapv_parts_are_numbered_alike_at_any_read_size() {
    let bytes = bytes_of(&[
        atom("sapv-form"),
        atom("latin1x3"),
        atom("status-indicator"),
        atom("decrara"),
    ]);
    let whole = drive_at(&bytes, bytes.len(), AmbiguousWidth::Wide);
    let one = drive_at(&bytes, 1, AmbiguousWidth::Wide);
    assert_eq!(whole, one);
}

/// **An alternative font and a variant whose bits equal its number are
/// numbered alike at any read size**: `SGR 11` is font 1 and SAPV 2 the
/// variants' first bit, `SGR 18` font 8 and SAPV 10 their fourth. While the
/// hash folded the two by exclusive or they cancelled, the rendition hashed
/// as the plain one, and the status line's indicator and DECRARA numbered
/// the pair in an order the reads decided. Bold with 10 is the control: it
/// shares no word with the variants.
#[test]
fn a_font_and_the_variant_its_number_equals_are_numbered_alike() {
    for bytes in [
        b"\x1b[11m\x1b[2 ]abc\x1b[1$~\x1b[1;2;3;7;7$t".as_slice(),
        b"\x1b[18m\x1b[10 ]abc\x1b[1$~\x1b[1;2;3;7;7$t".as_slice(),
        b"\x1b[1m\x1b[10 ]abc\x1b[1$~\x1b[1;2;3;7;7$t".as_slice(),
    ] {
        let whole = drive_at(bytes, bytes.len(), AmbiguousWidth::Wide);
        let one = drive_at(bytes, 1, AmbiguousWidth::Wide);
        assert!(whole == one, "{:?}", String::from_utf8_lossy(bytes));
    }
}

/// **`REP` repeats a form-taking run's last character**: pinned from
/// 200,000 cases. The run's first character, written alone for the form
/// it takes, recorded itself as the character `REP` repeats, over the
/// last one the run recorded, so a byte at a time and whole disagreed.
#[test]
fn rep_after_a_form_taking_run_repeats_its_last_character() {
    let bytes = bytes_of(&[atom("sapv-form"), atom("ascii"), atom("rep")]);
    assert_eq!(drive(&bytes, bytes.len()), drive(&bytes, 1));
}

/// **The paragraph-mode atoms reach the rows**, the control the path atoms
/// have: at a paragraph's start `box-mirror-on` and `autodetect-off` turn
/// the cursor's line, and a line feed carries both to the next.
#[test]
fn the_paragraph_mode_atoms_reach_the_rows() {
    use baud_domain::{Autodetect, BoxMirror};
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let bidi = |t: &Term<'_>, y: usize| t.row_extent(Row::from_index(y)).bidi();
    support::feed(
        &mut t,
        &bytes_of(&[atom("box-mirror-on"), atom("autodetect-off")]),
    );
    support::feed(&mut t, b"ab\r\n");
    for y in [0, 1] {
        assert_eq!(bidi(&t, y).box_mirror(), BoxMirror::On, "row {y}");
        assert_eq!(bidi(&t, y).autodetect(), Autodetect::Off, "row {y}");
    }
}

/// **The guarded-area atoms reach the guard**, the control the C1 atoms
/// have: `spa-epa-el` leaves `ab` guarded through EL 1, and `erm-el` after
/// it erases them and keeps the guard. An atom whose SPA never reached the
/// handler would guard nothing, and every property would compare cells with
/// no guard in them.
#[test]
fn the_guard_atoms_reach_the_guard() {
    let row = |cells: &[PackedCell]| {
        cells[..3]
            .iter()
            .map(|c| {
                let g = if c.flags.is_guarded() { 'G' } else { '.' };
                let ch = c
                    .content
                    .as_codepoint()
                    .map_or(' ', |c| char::from_u32(c.value()).unwrap_or(' '));
                format!("{ch}{g}")
            })
            .collect::<String>()
    };
    let (cells, ..) = drive(&bytes_of(&[atom("spa-epa-el")]), 4096);
    assert_eq!(row(&cells), "aGbG G");
    let (cells, ..) = drive(&bytes_of(&[atom("spa-epa-el"), atom("erm-el")]), 4096);
    assert_eq!(row(&cells), " G G G");
}

/// **The mode-list atom reaches its second mode**, the control the C1
/// atoms have: `sm-list` after `abc` and a carriage return inserts its `X`
/// in front of them, which only IRM, the list's second parameter, does.
#[test]
fn the_mode_list_atom_reaches_its_second_mode() {
    let mut b = b"abc\r".to_vec();
    b.extend_from_slice(&bytes_of(&[atom("sm-list")]));
    let (cells, ..) = drive(&b, 4096);
    let row: String = cells[..4]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |c| char::from_u32(c.value()).unwrap_or(' '))
        })
        .collect();
    assert_eq!(row, "Xabc");
}

/// **The erase-list atoms reach their last parameter**: after `abc` and
/// a step back, `el-list` empties the whole line, which only its `0` after
/// the `1` does, and `decsel-list` turns all three into spaces, which only
/// its `1` after the `0` does.
#[test]
fn the_erase_list_atoms_reach_their_last_parameter() {
    let row = |name: &str| {
        let mut b = b"abc\x1b[2D".to_vec();
        b.extend_from_slice(&bytes_of(&[atom(name)]));
        let (cells, ..) = drive(&b, 4096);
        cells[..4]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .map_or('.', |c| char::from_u32(c.value()).unwrap_or('?'))
            })
            .collect::<String>()
    };
    assert_eq!(row("el-list"), "....");
    assert_eq!(row("decsel-list"), "   .");
}

/// **The selective-erase atom reaches DECSEL**: after `ab` its erase turns
/// them into spaces, spares its protected `P` and leaves the empty position
/// after it empty.
#[test]
fn the_selective_erase_atom_reaches_decsel() {
    let mut b = b"ab".to_vec();
    b.extend_from_slice(&bytes_of(&[atom("decsca-decsel")]));
    let (cells, ..) = drive(&b, 4096);
    let row: String = cells[..4]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or('.', |c| char::from_u32(c.value()).unwrap_or('?'))
        })
        .collect();
    assert_eq!(row, "  P.");
}

/// **The `OSC 52` atoms reach the arms they were added for**, the control
/// the C1 atoms have: an atom whose body never reached the clipboard would
/// leave it empty in every chunking and green in every property below.
#[test]
fn the_osc52_atoms_reach_the_clipboard() {
    let one = |name: &str| drive(&bytes_of(&[atom(name)]), 4096).4.1;
    let c = Selections::NONE.push(b'c').resolve();
    assert_eq!(one("osc52-write"), Some((c, Some(b"hello world".to_vec()))));
    let p = Selections::NONE.push(b'p').resolve();
    assert_eq!(one("osc52-unpadded"), Some((p, Some(b"foob".to_vec()))));
    assert_eq!(one("osc52-clear"), Some((Selections::NONE.resolve(), None)));
    assert_eq!(one("osc52-cancelled"), None, "cancelled: nothing published");
    assert_eq!(one("osc52-read"), None, "a read publishes nothing");
    fed("osc52-read", &|t| {
        assert_eq!(t.clipboard_reads_refused, 1);
        assert!(t.reply().is_empty(), "and is answered with silence");
    });
}

/// **The TSR and HTJ atoms reach their functions**: `ab` justified
/// against the stop at 8, and against the one at 16 once TSR has removed
/// it; `89` does it under ISO 8859-1.
#[test]
fn the_tab_atoms_reach_tsr_and_htj() {
    let fed_then = |names: &[&str]| {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let prog: Vec<usize> = names.iter().map(|n| atom(n)).collect();
        support::feed(&mut t, &bytes_of(&prog));
        let text: String = t.row(Row::ZERO).expect("a row")[..16]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        (text, t.cursor().1)
    };
    let col = Col::at_index;
    assert_eq!(
        fed_then(&["ascii-short", "htj"]),
        ("......ab........".into(), col(8))
    );
    assert_eq!(
        fed_then(&["tsr", "ascii-short", "htj"]),
        ("..............ab".into(), col(16))
    );
    assert_eq!(
        fed_then(&["latin1", "ascii-short", "htj-8bit"]),
        ("......ab........".into(), col(8))
    );
}

/// **The SLH and SLL atoms reach the wrap**: the text wraps at the limit,
/// column 12, to the home, column 5.
#[test]
fn the_bound_atoms_reach_slh_and_sll() {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let prog: Vec<usize> = ["sll", "slh", "ascii"].iter().map(|n| atom(n)).collect();
    support::feed(&mut t, &bytes_of(&prog));
    let row = |y: usize| -> String {
        t.row(Row::from_index(y)).expect("a row")[..12]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect()
    };
    assert_eq!(row(0), "the quick br");
    assert_eq!(row(1), "....own fox.");
}

/// **The second limit, HPR and CHT atoms reach what they move**: the
/// text wraps at column 5, HPR moves on one column and CHT to the stop at
/// column 8.
#[test]
fn the_second_limit_hpr_and_cht_atoms_reach_the_cursor() {
    let w = ws();
    let run = |names: &[&str]| -> (String, Col) {
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let prog: Vec<usize> = names.iter().map(|n| atom(n)).collect();
        support::feed(&mut t, &bytes_of(&prog));
        let row: String = t.row(Row::from_index(1)).expect("a row")[..6]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        (row, t.cursor().1)
    };
    assert_eq!(run(&["sll-near", "ascii"]).0, "uick .");
    assert_eq!(run(&["ascii-short", "hpr"]).1, Col::at_index(3));
    assert_eq!(run(&["cht"]).1, Col::at_index(8));
}

/// **With DECAWM reset, a write past the line limit on a double line pins
/// to the half's last column**: a wide character, `\u{e9}` under the
/// wide policy, written on it from past SLL's limit put the cursor one
/// column right of the half's sentinel in one read, and a read beginning
/// on the status line pulled it back, so the atoms one at a time read 9
/// where the one read read 11. Found at 200,000 cases.
#[test]
fn a_write_pinned_past_the_limit_on_a_double_line_agrees_atom_by_atom() {
    let prog: Vec<usize> = [
        "status-host-on",
        "decawm-reset",
        "decdwl",
        "esc-inter-introducer",
        "csi-entry-lead",
        "hpr",
        "ascii",
        "sll-near",
        "latin1x3",
        "latin1-enter",
    ]
    .iter()
    .map(|n| atom(n))
    .collect();
    let w = AmbiguousWidth::Wide;
    let one = drive_at(&bytes_of(&prog), 4096, w);
    assert_eq!(one.7.2.as_ref().map(|l| l.1), Some(Some(Col::at_index(9))));
    assert_eq!(one, drive_atomwise(&prog, w));
}

/// **A second program the same pin closes**, found at 200,000 cases on
/// the base: the one read left the status line's cursor a column right
/// of where the atoms one at a time left it, under either policy.
#[test]
fn a_wide_run_past_the_limit_on_a_double_status_line_agrees_atom_by_atom() {
    let prog: Vec<usize> = [
        "decawm-reset",
        "decdhl-bottom",
        "apc-bel",
        "ppa",
        "srs-begin",
        "erm-el",
        "wide-three",
        "sll-near",
        "status-none",
        "decdhl-top",
        "dcsm-data",
        "decsel-list",
        "decrsps-cir",
        "decom-reset",
        "g3-graphics",
        "decdhl-bottom",
        "status-host-on",
        "hpa",
        "decdwl",
        "cuf",
        "htj",
        "dcs-bel",
        "wide-three",
        "tbc",
        "sds-end",
        "wide-three",
        "decawm-set",
        "ascii-short",
        "decswl",
    ]
    .iter()
    .map(|n| atom(n))
    .collect();
    for w in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        assert_eq!(
            drive_at(&bytes_of(&prog), 4096, w),
            drive_atomwise(&prog, w)
        );
    }
}

/// **DECLRMM set on the status line makes the line's own row single-width
/// too**, as it makes the main display's: found at 200,000 cases on the
/// base, with the alphabet 174 atoms long, where the line kept its
/// double-width row under DECLRMM, so `CNL` under DECRLM took the line's
/// start from the margin the one read had left, column 15 counted from
/// one, and from the half a read beginning on the line took again, column
/// 10, atom by atom.
#[test]
fn declrmm_on_a_double_status_line_agrees_atom_by_atom() {
    let prog: Vec<usize> = [
        "status-host-on",
        "decrlm-on",
        "decdwl",
        "decstbm-decslrm",
        "cnl",
        "latin1-dcs-osc",
    ]
    .iter()
    .map(|n| atom(n))
    .collect();
    for w in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        let one = drive_at(&bytes_of(&prog), 4096, w);
        assert_eq!(one.7.2.as_ref().map(|l| l.1), Some(Some(Col::at_index(14))));
        assert_eq!(one, drive_atomwise(&prog, w));
    }
}

/// **A margin mode reset on one display puts back the other's margins**,
/// the two programs the widened alphabet shrank to on the base in a debug
/// build: DECCOLM received on the status line, within 8,000 cases, and
/// with `decstr` and `deccolm` left out, DECLRMM reset there, a rare find
/// of the atom-at-a-time property alone. Each left the other display with
/// margins and the mode reset, and the next move tripped `Paint::place`'s
/// assertion (`declrmm_reset_on_either_display_puts_back_both_displays_margins`
/// holds what an optimised build showed).
#[test]
fn a_margin_mode_reset_on_one_display_agrees_atom_by_atom() {
    let programs: [&[&str]; 2] = [
        &[
            "decstbm-decslrm",
            "status-host-on",
            "declrmm-off",
            "status-main",
            "hpr",
        ],
        &["status-host-on", "decstbm-decslrm", "deccolm", "decfi"],
    ];
    for names in programs {
        let prog: Vec<usize> = names.iter().map(|n| atom(n)).collect();
        for w in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
            assert_eq!(
                drive_at(&bytes_of(&prog), 4096, w),
                drive_atomwise(&prog, w),
                "{names:?}"
            );
        }
    }
}

/// **The SEE atoms reach the extent**: under the page an `ICH` at the
/// start of a full first row pushes its last character onto the second.
#[test]
fn the_see_atoms_reach_the_extent() {
    let second = |names: &[&str]| {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let mut b = b"abcdefghijklmnopqrst".to_vec();
        let prog: Vec<usize> = names.iter().map(|n| atom(n)).collect();
        b.extend_from_slice(&bytes_of(&prog));
        b.extend_from_slice(b"\x1b[1;1H\x1b[@");
        support::feed(&mut t, &b);
        t.row(Row::from_index(1)).expect("a row")[0]
            .content
            .as_codepoint()
            .map(|c| c.value())
    };
    assert_eq!(second(&["see-page"]), Some(0x74));
    assert_eq!(second(&["see-page", "see-line"]), None);
}

/// **The aligned-stop atoms reach the layout**: `ab` after TALE's stop
/// at 9 ends before it.
#[test]
fn the_aligned_atoms_reach_the_layout() {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let prog: Vec<usize> = ["tale", "ht", "ascii-short"]
        .iter()
        .map(|n| atom(n))
        .collect();
    support::feed(&mut t, &bytes_of(&prog));
    let row: String = t.row(Row::ZERO).expect("a row")[..9]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('.')
        })
        .collect();
    assert_eq!(row, "......ab.");
}

/// **The page atoms reach the page functions**: NP takes the cursor home
/// from the end of the text, PPA leaves it there and is counted.
#[test]
fn the_page_atoms_reach_np_and_ppa() {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let before = t.pages_missing();
    let prog: Vec<usize> = ["ascii", "ppa"].iter().map(|n| atom(n)).collect();
    support::feed(&mut t, &bytes_of(&prog));
    assert_eq!(t.cursor().1.index(), 19);
    assert_ne!(t.pages_missing(), before);
    support::feed(&mut t, &bytes_of(&[atom("np")]));
    assert_eq!((t.cursor().0.index(), t.cursor().1.index()), (0, 0));
}

proptest! {
    /// Where the reads fall must not change the screen.
    ///
    /// **Every cell, not row 0**, and over the alphabet above rather than one
    /// hand-written line. The test this replaces compared row 0 of a four-row
    /// screen for a payload with no zero-width character and no run longer
    /// than two -- so it could not reach either defect it was later credited
    /// with being able to catch.
    ///
    /// Under both ambiguous-width policies, which move every box-drawing
    /// atom's width.
    #[test]
    fn the_screen_does_not_depend_on_where_the_reads_fell(prog in program(), a in 1usize..=64, w in policy()) {
        let b = bytes_of(&prog);
        prop_assert_eq!(drive_at(&b, a, w), drive_at(&b, 4096, w));
    }

    /// A run and the same characters fed one atom at a time must agree.
    ///
    /// The second mechanism: chunk invariance says two *chunkings* agree and
    /// is silent when both are wrong the same way, which is how a blitter that
    /// clipped at the margin passed it at every read size.
    #[test]
    fn a_program_agrees_with_the_same_atoms_fed_one_at_a_time(prog in program(), w in policy()) {
        let b = bytes_of(&prog);
        prop_assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
    }

    /// **Every linked cell is on a row a link-table sweep reads**
    /// (`Term::links_unread`): written by text, a rectangle's copy or
    /// `DECALN`, carried by `IL`, `DL`, a scroll inside margins or a screen
    /// switch, and across a narrowing and a growth back. A cell a sweep does
    /// not read is one whose slot it may give away.
    #[test]
    fn every_linked_cell_is_on_a_row_a_sweep_reads(prog in program(), cut in 0usize..40) {
        let w = ws();
        let mut owned = Owned::new(&w);
        let mut t = Term::new(owned.storage(), &w);
        let cut = cut.min(prog.len());
        let feed = |t: &mut Term<'_>, p: &[usize]| {
            let b = bytes_of(p);
            t.write_at(BytePos::ZERO, &b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        feed(&mut t, &prog[..cut]);
        prop_assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "after {} atoms", cut);
        // Three rows and seven columns fewer than `ws`.
        let narrow = WinSize { ws_row: 47, ws_col: 13 };
        prop_assert_eq!(t.resize(&narrow), Resized::Ok);
        feed(&mut t, &prog[cut..]);
        prop_assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "narrowed");
        prop_assert_eq!(t.resize(&w), Resized::Ok);
        prop_assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "grown back");
        feed(&mut t, &prog);
        prop_assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "after all again");
    }

    /// **A cell that claims continuations must have them.**
    ///
    /// The one invariant here with no oracle behind it: everything else in
    /// this repo compares two implementations or two chunkings. A cluster
    /// index that has gone stale answers *empty* rather than wrong -- by
    /// design, because the store checks the base -- so this is what turns that
    /// silence into a failure. It is also what discriminates content-keying
    /// from position-keying: `baud-core2`'s marks table could not pass this
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

/// **The prompt-mark atoms reach both readings of an initial mark**, so the
/// chunk properties above put the new rule under every read size: a
/// prompt, its input, a line below it and an `A` there is the prompt drawn
/// again, erasing nothing and opening no second command; an `A` with the
/// cursor still on the input's line is a redraw, which erases.
#[test]
fn the_prompt_mark_atoms_reach_a_redraw_and_a_prompt_drawn_again() {
    let again = bytes_of(&[
        atom("osc133_a"),
        atom("ascii-short"),
        atom("osc133_b"),
        atom("ascii-short"),
        atom("crlf"),
        atom("ascii-short"),
        atom("crlf"),
        atom("osc133_a"),
    ]);
    support::with_term(&ws(), |t| {
        support::feed(t, &again);
        assert_eq!(t.prompt_erases, 0, "the prompt drawn again");
        assert_eq!(t.prompts().count(), 1, "one command");
        assert_eq!(t.cursor().0, Row::from_index(2));
    });
    let redraw = bytes_of(&[
        atom("osc133_a"),
        atom("ascii-short"),
        atom("osc133_b"),
        atom("ascii-short"),
        atom("osc133_a"),
    ]);
    support::with_term(&ws(), |t| {
        support::feed(t, &redraw);
        assert_eq!(t.prompt_erases, 1, "a redraw");
        assert_eq!(t.cursor().0, Row::ZERO);
    });
}

/// The content of screen cell `(y, x)`.
fn content_at(t: &Term<'_>, y: usize, x: usize) -> Content {
    t.row(Row::from_index(y)).expect("a row")[x].content
}

/// **The erase atoms reach their operations**, each with the positive
/// control the C1 atoms have: a written row, then the atom, then what it
/// erased read back. An atom that silently reached nothing would leave the
/// write-generation property below green over stores it never made.
#[test]
fn the_erase_atoms_reach_their_operations() {
    let a = Content::codepoint(Codepoint::from_char('a'));
    // ED from the cursor, at column 1 of the written row: the cursor's cell
    // and the rest go, the cell before it stays.
    fed_after(b"abc\x1b[1;2H", "ed", &|t| {
        assert_eq!(content_at(t, 0, 0), a);
        assert!(content_at(t, 0, 1).is_empty() && content_at(t, 0, 2).is_empty());
    });
    // ED 1 to the cursor, inclusive: column 2 stays.
    fed_after(b"abc\x1b[1;2H", "ed1", &|t| {
        assert!(content_at(t, 0, 0).is_empty() && content_at(t, 0, 1).is_empty());
        assert!(!content_at(t, 0, 2).is_empty());
    });
    fed_after(b"abc\r\nxyz", "ed2", &|t| {
        assert!((0..3).all(|x| content_at(t, 0, x).is_empty() && content_at(t, 1, x).is_empty()));
    });
    // ECH 3 from column 1: columns 1 to 3 go, 0 and 4 stay.
    fed_after(b"abcde\x1b[1;2H", "ech", &|t| {
        assert_eq!(content_at(t, 0, 0), a);
        assert!((1..4).all(|x| content_at(t, 0, x).is_empty()));
        assert!(!content_at(t, 0, 4).is_empty());
    });
    // DECSED spares the cell DECSCA protected and erases the other, to the
    // space its erased form is.
    fed_after(b"a\x1b[1\"qb\x1b[0\"q\x1b[H", "decsed", &|t| {
        assert_ne!(content_at(t, 0, 0), a);
        assert_eq!(
            content_at(t, 0, 1),
            Content::codepoint(Codepoint::from_char('b'))
        );
    });
    // Sixty lines on a fifty-row screen leave history; ED 3 erases it.
    let history: Vec<u8> = (0..60).flat_map(|_| *b"hist\r\n").collect();
    let (none, fresh) = support::with_term(&ws(), |t| (t.history_erasures(), t.resets()));
    fed_after(&history, "ed3", &|t| {
        assert_eq!(t.scrollback().len(), 0, "the history is gone");
        assert_ne!(t.history_erasures(), none, "history_erasures rises");
    });
    // DECSED's form of ED 3 turns the history's erasable characters into
    // spaces in their own rendition, and keeps the rows.
    fed_after(&history, "decsed3", &|t| {
        let sb = t.scrollback();
        assert!(!sb.is_empty(), "the rows stay");
        let (line, _) = sb.line(baud_domain::Back::of(0)).expect("a line");
        let erased = PackedCell::selectively_erased(PackedCell::glyph(
            Codepoint::from_char('h'),
            baud_domain::StyleId::DEFAULT,
            baud_domain::CellFlags::NONE,
        ));
        assert_eq!(
            line[0], erased,
            "the history's cells read DECSED's erased form"
        );
    });
    fed("ris", &|t| {
        assert_ne!(t.resets(), fresh, "a full reset is counted")
    });
}

/// One snapshot of every physical row of the three surfaces: its cells at
/// the pitch and its mark.
type Rows3 = Vec<(Which, usize, Vec<PackedCell>, RowExtent)>;

fn rows3(t: &Term<'_>, kept: &Seal) -> (Rows3, std::collections::BTreeSet<(usize, usize)>) {
    let mut all = Vec::new();
    let mut written = std::collections::BTreeSet::new();
    for (k, w) in [Which::Primary, Which::Alternate, Which::Status]
        .into_iter()
        .enumerate()
    {
        for (at, cells, mark, dirt) in t.stored_rows(w, kept) {
            all.push((w, at.p.index(), cells.to_vec(), mark));
            if dirt == RowDirt::Written {
                written.insert((k, at.p.index()));
            }
        }
    }
    (all, written)
}

proptest! {
    /// **Every store moves the row it touches** (the core write generation):
    /// fed in reads split where the generator says -- mid-character and
    /// mid-sequence included -- with a seal after every read, every
    /// physical row of the primary ring, the alternate screen and the
    /// status line is read back after each read, and
    ///
    /// 1. a row whose cells or mark differ from the last read is written
    ///    against the last seal;
    /// 2. any such row means [`Term::wrote_since`] says so;
    /// 3. a row written against a kept seal stays written against it at
    ///    every later read, for the last eight seals.
    ///
    /// A store that skipped its stamp leaves a changed row reading clean,
    /// which a renderer keeping its copy would draw stale. A history of
    /// twenty lines, so the ring wraps under a program and a snapshot stays
    /// cheap.
    #[test]
    fn writes_move_the_rows_they_touch(prog in program(), cuts in proptest::collection::vec(0usize..4096, 0..8)) {
        let b = bytes_of(&prog);
        let mut at: Vec<usize> = cuts.iter().map(|c| c % (b.len() + 1)).collect();
        at.push(0);
        at.push(b.len());
        at.sort_unstable();
        at.dedup();
        let w = ws();
        let mut owned = Owned::with_history(&w, &CacheGeometry::UNKNOWN, Depth::of(20)).with_clipboard(CLIP);
        let mut t = Term::new(owned.storage(), &w);
        let mut kept: std::collections::VecDeque<(Seal, std::collections::BTreeSet<(usize, usize)>)> =
            std::collections::VecDeque::new();
        let mut last = t.seal_writes();
        let (mut before, _) = rows3(&t, &last);
        for pair in at.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            t.write_at(BytePos::ZERO, &b[lo..hi]);
            t.feed(BytePos::ZERO, ReadLen::from_index(hi - lo));
            let (now, written) = rows3(&t, &last);
            let mut changed = 0;
            for (old, new) in before.iter().zip(&now) {
                prop_assert_eq!((old.0, old.1), (new.0, new.1));
                if old.2 != new.2 || old.3 != new.3 {
                    changed += 1;
                    let k = [Which::Primary, Which::Alternate, Which::Status]
                        .iter()
                        .position(|w| *w == new.0)
                        .unwrap();
                    prop_assert!(
                        written.contains(&(k, new.1)),
                        "{:?} row {} changed in read {}..{} and reads clean",
                        new.0, new.1, lo, hi
                    );
                }
            }
            prop_assert!(changed == 0 || t.wrote_since(&last), "rows changed and wrote_since is false");
            for (seal, was) in kept.iter_mut() {
                let (_, still) = rows3(&t, seal);
                prop_assert!(was.is_subset(&still), "a row written against a kept seal reads clean again");
                *was = still;
            }
            kept.push_back((last, written));
            if kept.len() > 8 {
                kept.pop_front();
            }
            last = t.seal_writes();
            before = now;
        }
    }
}

// Programs the properties above once shrank to, each pinned as a test and
// spelled by atom name, so an atom added to the alphabet never moves it.

/// Wide text, a combining mark, box drawing and ASCII up to the margin,
/// `DECAWM` reset, then a flag and ASCII that pin at the last column: fed
/// whole and atom by atom the screen agrees, and every cell that claims
/// marks resolves to some.
#[test]
fn a_mark_and_a_flag_pinned_at_the_margin_agree_atom_by_atom_and_resolve() {
    let prog = [
        atom("wide-three"),
        atom("wide-three"),
        atom("combining"),
        atom("narrow-quad"),
        atom("ascii-short"),
        atom("ascii"),
        atom("combining"),
        atom("ascii"),
        atom("decawm-reset"),
        atom("flag"),
        atom("ascii"),
    ];
    let b = bytes_of(&prog);
    let a = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, a), drive_atomwise(&prog, a));
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    t.write_at(BytePos::ZERO, &b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    let stride = t.stride().index();
    let cells = support::screen(&t);
    for y in 0..ROWS {
        for x in 0..COLS {
            if cells[y * stride + x].flags.is_marked() {
                let n = t.marks(Row::from_index(y), Col::at_index(x)).count();
                assert!(n > 0, "cell ({y},{x}) claims marks and resolves to none");
            }
        }
    }
}

/// Latin-1 strings ended by NEL and CSI, marks after wide and zero-width
/// text, renditions, a hidden cursor, `CUU` and `DECAWM` reset: read a byte
/// at a time it is the screen read whole.
#[test]
fn latin1_strings_marks_and_renditions_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("latin1-osc-nel"),
        atom("wide-quad"),
        atom("latin1-osc-nel"),
        atom("combining"),
        atom("cursor-hide"),
        atom("ascii-short"),
        atom("latin1x3"),
        atom("sgr"),
        atom("zero-in-three"),
        atom("cursor-hide"),
        atom("cuu"),
        atom("latin1-csi-osc"),
        atom("combining"),
        atom("sgr"),
        atom("cursor-hide"),
        atom("sgr"),
        atom("sgr"),
        atom("ascii-short"),
        atom("sgr"),
        atom("sgr"),
        atom("decawm-reset"),
        atom("sgr"),
        atom("cursor-hide"),
        atom("ascii-short"),
        atom("zero-in-three"),
        atom("combining"),
        atom("sgr"),
        atom("combining"),
        atom("latin1-dcs-osc"),
        atom("flag"),
        atom("latin1-osc-csi"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// `DECAWM` reset, nineteen letters and `e U+0301` on twenty columns: the
/// `e` lands in the last column and the mark joins it, fed whole and atom by
/// atom alike, and every cell that claims marks resolves to some.
#[test]
fn a_mark_after_a_letter_in_the_last_column_agrees_atom_by_atom_and_resolves() {
    let prog = [atom("decawm-reset"), atom("ascii"), atom("combining")];
    let b = bytes_of(&prog);
    let a = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, a), drive_atomwise(&prog, a));
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    t.write_at(BytePos::ZERO, &b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    let stride = t.stride().index();
    let cells = support::screen(&t);
    for y in 0..ROWS {
        for x in 0..COLS {
            if cells[y * stride + x].flags.is_marked() {
                let n = t.marks(Row::from_index(y), Col::at_index(x)).count();
                assert!(n > 0, "cell ({y},{x}) claims marks and resolves to none");
            }
        }
    }
}

/// Latin-1 escapes and strings, wide text with marks around `DECAWM` reset,
/// C1-ended strings under UTF-8, box drawing, homes and a hidden cursor:
/// read a byte at a time it is the screen read whole.
#[test]
fn wide_marks_past_decawm_reset_and_c1_strings_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("latin1-esc-inter-csi"),
        atom("latin1-osc-nel"),
        atom("latin1-osc-csi"),
        atom("wide-three"),
        atom("combining"),
        atom("decawm-reset"),
        atom("wide-three"),
        atom("combining"),
        atom("wide-quad"),
        atom("latin1x3"),
        atom("zero-in-three"),
        atom("osc-c1-utf8"),
        atom("narrow-quad"),
        atom("narrow-quad"),
        atom("osc-c1-utf8"),
        atom("cup-home"),
        atom("cursor-hide"),
        atom("cup-home"),
        atom("ascii"),
        atom("combining"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// An emoji pair, Latin-1 letters and `DECSACE` 0: read a byte at a time it
/// is the screen read whole.
#[test]
fn an_emoji_pair_latin1_and_decsace_read_a_byte_at_a_time_agree() {
    let prog = [atom("emoji-pair"), atom("latin1x3"), atom("decsace-stream")];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// Box drawing, wide text and `DECSACE` 0: fed whole and atom by atom the
/// screen agrees.
#[test]
fn box_drawing_wide_text_and_decsace_agree_atom_by_atom() {
    let prog = [
        atom("narrow-quad"),
        atom("wide-quad"),
        atom("decsace-stream"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// `REP` after a zero-width character, then cancelled and ignored control
/// sequences, a Latin-1 string, `DECSACE` 0, `DECRQCRA` and a mode list,
/// under wide ambiguous width: fed whole and atom by atom the screen agrees.
#[test]
fn rep_after_a_zero_width_character_and_cancelled_sequences_agree_atom_by_atom() {
    let prog = [
        atom("zero-in-three"),
        atom("rep"),
        atom("decsel-list"),
        atom("csi-cancelled"),
        atom("csi-cancelled"),
        atom("latin1-osc-csi"),
        atom("csi-entry-lead"),
        atom("decsace-stream"),
        atom("csi-ignore-cont"),
        atom("decrqcra"),
        atom("sm-list"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Wide;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// `SAPV`'s form selection, Latin-1 letters, an indicator status line and
/// `DECRARA`, under wide ambiguous width: fed whole and atom by atom the
/// screen agrees.
#[test]
fn sapv_latin1_and_a_status_line_with_decrara_agree_atom_by_atom() {
    let prog = [
        atom("sapv-form"),
        atom("latin1x3"),
        atom("status-indicator"),
        atom("decrara"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Wide;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// Latin-1 strings, a mark, `DECSACE` 2, `SAPV`'s form selection, `DECBI`,
/// line ends, ASCII and `REP`: read a byte at a time it is the screen read
/// whole.
#[test]
fn sapv_decbi_and_rep_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("latin1-osc-csi"),
        atom("latin1-osc-nel"),
        atom("ascii-short"),
        atom("combining"),
        atom("decsace-rectangle"),
        atom("ascii-short"),
        atom("sapv-form"),
        atom("decbi"),
        atom("crlf"),
        atom("crlf"),
        atom("ascii"),
        atom("rep"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// A font selected by its digits, an indicator status line, Latin-1
/// letters, ReGIS drawing, ASCII and `DECRARA`: read a byte at a time it is
/// the screen read whole.
#[test]
fn a_font_a_status_line_and_regis_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("sgr-font-digits"),
        atom("status-indicator"),
        atom("latin1x3"),
        atom("regis-draw"),
        atom("ascii"),
        atom("ascii"),
        atom("decrara"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// A font selected by its digits, an indicator status line, a control
/// sequence with a three-byte character inside it and `DECRARA`: fed whole
/// and atom by atom the screen agrees.
#[test]
fn a_font_a_status_line_and_decrara_agree_atom_by_atom() {
    let prog = [
        atom("sgr-font-digits"),
        atom("status-indicator"),
        atom("csi-entry-lead"),
        atom("decrara"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// A right-to-left `SDS` string, `SAPV`'s held selections, an alternative
/// font, an indicator status line and an underlined `SDS`: fed whole and
/// atom by atom the screen agrees.
#[test]
fn sds_sapv_and_a_status_line_agree_atom_by_atom() {
    let prog = [
        atom("sds-rtl"),
        atom("sapv-held"),
        atom("sgr-font"),
        atom("status-indicator"),
        atom("sgr-underline-sds"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// `SAPV`'s held selections, an indicator status line, a right-to-left
/// `SDS` string, an alternative font, ReGIS drawing and an underlined
/// `SDS`: read a byte at a time it is the screen read whole.
#[test]
fn sapv_a_status_line_sds_and_regis_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("sapv-held"),
        atom("status-indicator"),
        atom("sds-rtl"),
        atom("sgr-font"),
        atom("regis-draw"),
        atom("sgr-underline-sds"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// A status line the host writes, `DECRLM` set, a double-width line, an
/// incomplete character after a prepend, `DECCIR` asked for and a Latin-1
/// string ended by `CSI`: read a byte at a time it is the screen read whole.
#[test]
fn a_status_line_under_decrlm_on_a_double_width_line_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("status-host-on"),
        atom("decrlm-on"),
        atom("decdwl"),
        atom("fffd-cut-after-prepend"),
        atom("deccir"),
        atom("latin1-osc-csi"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// `DECAWM` reset, a status line the host writes, a double-width line,
/// `DECRLM` set, a control sequence with a three-byte character inside it
/// and a Latin-1 escape with an intermediate: fed whole and atom by atom the
/// screen agrees.
#[test]
fn a_status_line_under_decrlm_on_a_double_width_line_agrees_atom_by_atom() {
    let prog = [
        atom("decawm-reset"),
        atom("status-host-on"),
        atom("decdwl"),
        atom("decrlm-on"),
        atom("csi-entry-lead"),
        atom("latin1-esc-inter-csi"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// Wide text on a double-width line, `CUU`, wide and narrow text with
/// zero-width characters inside it, line ends, a rendition, `DECCRA`, then
/// `IRM` set and Latin-1 letters inserted: read a byte at a time it is the
/// screen read whole.
#[test]
fn a_double_width_line_deccra_and_irm_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("wide-quad"),
        atom("decdwl"),
        atom("cuu"),
        atom("ascii-short"),
        atom("wide-quad"),
        atom("wide-quad"),
        atom("narrow-quad"),
        atom("zero-in-quad"),
        atom("crlf"),
        atom("zero-in-quad"),
        atom("sgr"),
        atom("deccra"),
        atom("ascii-short"),
        atom("irm-on"),
        atom("latin1x3"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// `IRM` set, two letters, `HPA` back to the second column, then wide and
/// private-use runs inserted before them: fed whole and atom by atom the
/// screen agrees.
#[test]
fn runs_inserted_under_irm_after_hpa_agree_atom_by_atom() {
    let prog = [
        atom("irm-on"),
        atom("ascii-short"),
        atom("hpa"),
        atom("wide-quad"),
        atom("pua-quad"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// `OSC 52` writes, marks, `DECSACE` 2, a status line, a right-to-left
/// `SCP`, ReGIS, `EL` with a list, `DECBI`, `DCSM` at DATA with `BDSM`
/// reset, `REP`, a double-width line, `DECRSPS`, `DECFRA`, `SS2`, a flag and
/// `SLL` near the edge, then Latin-1 and ASCII: read 26 bytes at a time it
/// is the screen read whole.
#[test]
fn a_line_limit_on_a_double_width_line_read_26_bytes_at_a_time_agree() {
    let prog = [
        atom("latin1x3"),
        atom("latin1-osc-nel"),
        atom("ascii-short"),
        atom("ascii-short"),
        atom("osc52-unpadded"),
        atom("combining"),
        atom("decsace-rectangle"),
        atom("status-host-on"),
        atom("scp-rtl"),
        atom("osc52-write"),
        atom("regis-resync-display"),
        atom("el-list"),
        atom("decbi"),
        atom("combining"),
        atom("dcsm-data"),
        atom("bdsm-explicit"),
        atom("ascii-short"),
        atom("ascii-short"),
        atom("rep"),
        atom("rep"),
        atom("rep"),
        atom("combining"),
        atom("decdwl"),
        atom("decrsps-cir"),
        atom("decfra"),
        atom("ss2"),
        atom("decbi"),
        atom("flag"),
        atom("decsace-rectangle"),
        atom("sll-near"),
        atom("latin1-enter"),
        atom("ascii"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 26, w), drive_at(&b, 4096, w));
}

/// A status line the host writes, private-use runs either side of a
/// double-width line, `SLL` near the edge and ISO 8859-1 selected: fed
/// whole and atom by atom the screen agrees.
#[test]
fn a_line_limit_on_a_double_width_status_line_agrees_atom_by_atom() {
    let prog = [
        atom("status-host-on"),
        atom("pua-quad"),
        atom("decdwl"),
        atom("pua-quad"),
        atom("sll-near"),
        atom("latin1-enter"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 4096, w), drive_atomwise(&prog, w));
}

/// `every_linked_cell_is_on_a_row_a_sweep_reads` on one program and one
/// cut: the atoms before `cut`, a narrowing to 47x13, the rest, a growth
/// back and the whole program again leave no linked cell a sweep does not
/// read.
fn every_linked_cell_is_read_after(prog: &[usize], cut: usize) {
    let w = ws();
    let mut owned = Owned::new(&w);
    let mut t = Term::new(owned.storage(), &w);
    let cut = cut.min(prog.len());
    let feed = |t: &mut Term<'_>, p: &[usize]| {
        let b = bytes_of(p);
        t.write_at(BytePos::ZERO, &b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    };
    feed(&mut t, &prog[..cut]);
    assert_eq!(
        t.links_unread(),
        baud_domain::Cells::of(0),
        "after {cut} atoms"
    );
    let narrow = WinSize {
        ws_row: 47,
        ws_col: 13,
    };
    assert_eq!(t.resize(&narrow), Resized::Ok);
    feed(&mut t, &prog[cut..]);
    assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "narrowed");
    assert_eq!(t.resize(&w), Resized::Ok);
    assert_eq!(t.links_unread(), baud_domain::Cells::of(0), "grown back");
    feed(&mut t, prog);
    assert_eq!(
        t.links_unread(),
        baud_domain::Cells::of(0),
        "after all again"
    );
}

/// Linked ASCII copied by `DECCRA`, the whole program before the narrowing:
/// every linked cell is on a row a sweep reads.
#[test]
fn a_link_deccra_copies_is_on_a_row_a_sweep_reads() {
    let prog = [
        atom("osc8-open"),
        atom("ascii"),
        atom("osc8-close"),
        atom("deccra"),
    ];
    every_linked_cell_is_read_after(&prog, 4);
}

/// A link opened, `DECRSPS` restoring the cursor, GR bytes, margins on both
/// axes and `SD` scrolling inside them, the whole program before the
/// narrowing: every linked cell is on a row a sweep reads.
#[test]
fn a_link_sd_scrolls_inside_margins_is_on_a_row_a_sweep_reads() {
    let prog = [
        atom("osc8-open"),
        atom("decrsps-cir"),
        atom("gr-bytes"),
        atom("decstbm-decslrm"),
        atom("sd"),
    ];
    every_linked_cell_is_read_after(&prog, 5);
}

/// A status line the host writes, margins on both axes, `DECCOLM` received
/// on the status line and `HPR`: read a byte at a time it is the screen
/// read whole.
#[test]
fn deccolm_on_the_status_line_under_margins_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("status-host-on"),
        atom("decstbm-decslrm"),
        atom("deccolm"),
        atom("hpr"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// `BDSM` reset, a right-to-left `SDS` string, ASCII, margins on both axes,
/// `IRM` set, Latin-1, private-use and zero-width text inserted, `CBT` and
/// an escape with an intermediate: read a byte at a time it is the screen
/// read whole.
#[test]
fn text_inserted_under_irm_past_the_margins_right_to_left_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("bdsm-explicit"),
        atom("sds-rtl"),
        atom("ascii"),
        atom("decstbm-decslrm"),
        atom("irm-on"),
        atom("latin1x3"),
        atom("pua-quad"),
        atom("zero-in-three"),
        atom("cbt"),
        atom("esc-inter-introducer"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

/// `SLL` 12, margins on both axes, then the status line selected and `BDSM`
/// reset on it, the read ending there: read a byte at a time it is the
/// screen read whole.
#[test]
fn bdsm_reset_on_the_status_line_under_margins_and_a_limit_read_a_byte_at_a_time_agree() {
    let prog = [
        atom("sll"),
        atom("decstbm-decslrm"),
        atom("status-host-on"),
        atom("bdsm-explicit"),
    ];
    let b = bytes_of(&prog);
    let w = AmbiguousWidth::Narrow;
    assert_eq!(drive_at(&b, 1, w), drive_at(&b, 4096, w));
}

// The reach check: every done row of docs/vt-coverage.md against the bytes
// the alphabet can send. The document is read into keys, each atom's bytes
// are lexed into keys by a parser written here rather than baud's, and a row
// is reached when every key it names is sent.

/// What a row names and what a byte string sends, in one vocabulary.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Sent {
    /// A C0 control, executed.
    C0(u8),
    /// An 8-bit C1 control, executed under ISO 8859-1.
    C1(u8),
    /// An escape sequence: its intermediates, and its final or any final.
    Esc(Vec<u8>, Option<u8>),
    /// A control sequence: its form, its parameter count where the row
    /// spells more than one parameter, and its first parameter where the
    /// row spells it as a number.
    Csi(Form, Option<usize>, Option<u16>),
    /// A mode set or reset, with its marker.
    Mode(Option<u8>, u16),
    /// A rendition parameter and what follows it.
    Sgr(u16, SgrSub),
    /// An operating system command: its code and its first word.
    Osc(u32, Option<String>),
    /// A device control string's form.
    Dcs(Form),
    /// A graphics command's key and the first byte of its value.
    Apc(u8, Option<u8>),
    /// A parser state entered.
    State(Phase),
}

/// What follows a rendition parameter.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum SgrSub {
    /// Nothing the row names.
    Bare,
    /// A selector in the next parameter, after a semicolon: `38;5;n`.
    Semi(u16),
    /// A first sub-parameter, after a colon: `4:3`, `38:2::r:g:b`.
    Colon(u16),
}

/// The parser states of `[[source: williams]]`, by the names the document
/// gives them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Phase {
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    DcsEntry,
    DcsParam,
    DcsIntermediate,
    DcsPassthrough,
    DcsIgnore,
    OscString,
    SosPmApcString,
}

impl Phase {
    const ALL: [Phase; 14] = [
        Phase::Ground,
        Phase::Escape,
        Phase::EscapeIntermediate,
        Phase::CsiEntry,
        Phase::CsiParam,
        Phase::CsiIntermediate,
        Phase::CsiIgnore,
        Phase::DcsEntry,
        Phase::DcsParam,
        Phase::DcsIntermediate,
        Phase::DcsPassthrough,
        Phase::DcsIgnore,
        Phase::OscString,
        Phase::SosPmApcString,
    ];

    fn name(self) -> &'static str {
        match self {
            Phase::Ground => "ground",
            Phase::Escape => "escape",
            Phase::EscapeIntermediate => "escape_intermediate",
            Phase::CsiEntry => "csi_entry",
            Phase::CsiParam => "csi_param",
            Phase::CsiIntermediate => "csi_intermediate",
            Phase::CsiIgnore => "csi_ignore",
            Phase::DcsEntry => "dcs_entry",
            Phase::DcsParam => "dcs_param",
            Phase::DcsIntermediate => "dcs_intermediate",
            Phase::DcsPassthrough => "dcs_passthrough",
            Phase::DcsIgnore => "dcs_ignore",
            Phase::OscString => "osc_string",
            Phase::SosPmApcString => "sos_pm_apc_string",
        }
    }

    /// The states that collect a control string until `ST`.
    fn is_string(self) -> bool {
        matches!(
            self,
            Phase::OscString | Phase::SosPmApcString | Phase::DcsPassthrough | Phase::DcsIgnore
        )
    }
}

/// The encoding the bytes are read in, switched by `ESC % @` and `ESC % G`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Enc {
    Utf8,
    Latin1,
}

/// The control string being collected, for what its end dispatches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strand {
    Osc,
    Apc,
    Other,
}

/// Whether an `ESC` inside a control string waits for its `\`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pending {
    Nothing,
    Esc,
}

/// How a control string ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum End {
    Dispatch,
    Cancel,
}

/// The parser, keyed: each dispatch and each state entered becomes a
/// [`Sent`]. It reads one byte string from power-on, in UTF-8.
struct Lexer {
    out: BTreeSet<Sent>,
    phase: Phase,
    enc: Enc,
    inters: Vec<u8>,
    marker: Option<u8>,
    params: Vec<u8>,
    payload: Vec<u8>,
    strand: Option<Strand>,
    pending: Pending,
}

/// Everything `bytes` sends, read from power-on.
fn lex(bytes: &[u8]) -> BTreeSet<Sent> {
    let mut l = Lexer::fresh();
    for &b in bytes {
        l.byte(b);
    }
    l.out
}

/// The first parameter of a field, before any colon.
fn head(field: &str) -> &str {
    field.split(':').next().unwrap_or("")
}

/// A decimal parameter, held at the ceiling of a `u16`.
fn saturating(digits: &str) -> u16 {
    let v = digits
        .bytes()
        .filter(u8::is_ascii_digit)
        .fold(0u32, |v, c| (v * 10 + u32::from(c - b'0')).min(65_535));
    u16::try_from(v).unwrap_or(u16::MAX)
}

impl Lexer {
    fn fresh() -> Self {
        let mut out = BTreeSet::new();
        out.insert(Sent::State(Phase::Ground));
        Self {
            out,
            phase: Phase::Ground,
            enc: Enc::Utf8,
            inters: Vec::new(),
            marker: None,
            params: Vec::new(),
            payload: Vec::new(),
            strand: None,
            pending: Pending::Nothing,
        }
    }

    fn enter(&mut self, p: Phase) {
        self.phase = p;
        self.out.insert(Sent::State(p));
    }

    fn clear(&mut self) {
        self.inters.clear();
        self.marker = None;
        self.params.clear();
    }

    fn start(&mut self, s: Strand) {
        self.strand = Some(s);
        self.payload.clear();
    }

    fn end_string(&mut self, end: End) {
        if end == End::Dispatch {
            match self.strand {
                Some(Strand::Osc) => osc_sent(&self.payload, &mut self.out),
                Some(Strand::Apc) => apc_sent(&self.payload, &mut self.out),
                Some(Strand::Other) | None => {}
            }
        }
        self.strand = None;
        self.payload.clear();
    }

    /// An 8-bit C1 control under ISO 8859-1, from any state: a control
    /// string it arrives in is cancelled, or ended by `9C`.
    fn c1(&mut self, b: u8) {
        if self.phase.is_string() {
            self.end_string(if b == 0x9c {
                End::Dispatch
            } else {
                End::Cancel
            });
        }
        self.clear();
        match b {
            0x90 => self.enter(Phase::DcsEntry),
            0x9b => self.enter(Phase::CsiEntry),
            0x9d => {
                self.enter(Phase::OscString);
                self.start(Strand::Osc);
            }
            0x98 | 0x9e | 0x9f => {
                self.enter(Phase::SosPmApcString);
                self.start(if b == 0x9f {
                    Strand::Apc
                } else {
                    Strand::Other
                });
            }
            0x9c => self.enter(Phase::Ground),
            _ => {
                self.out.insert(Sent::C1(b));
                self.enter(Phase::Ground);
            }
        }
    }

    fn esc_dispatch(&mut self, f: u8) {
        self.out.insert(Sent::Esc(self.inters.clone(), Some(f)));
        if !self.inters.is_empty() {
            self.out.insert(Sent::Esc(self.inters.clone(), None));
        }
        if self.inters == b"%" {
            match f {
                b'@' => self.enc = Enc::Latin1,
                b'G' => self.enc = Enc::Utf8,
                _ => {}
            }
        }
    }

    fn csi_dispatch(&mut self, fin: u8) {
        let form = Form::of(self.marker, self.inters.last().copied(), fin);
        let text = String::from_utf8_lossy(&self.params).into_owned();
        let fields: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.split(';').collect()
        };
        let first = fields
            .first()
            .copied()
            .map(head)
            .filter(|h| !h.is_empty())
            .map(saturating);
        for arity in [None, Some(fields.len())] {
            self.out.insert(Sent::Csi(form, arity, None));
            if first.is_some() {
                self.out.insert(Sent::Csi(form, arity, first));
            }
        }
        let plain = self.inters.is_empty();
        if plain && matches!(fin, b'h' | b'l') && matches!(self.marker, None | Some(b'?')) {
            for f in &fields {
                let h = head(f);
                if !h.is_empty() {
                    self.out.insert(Sent::Mode(self.marker, saturating(h)));
                }
            }
        }
        if plain && fin == b'm' && self.marker.is_none() {
            sgr_sent(&fields, &mut self.out);
        }
    }

    fn byte(&mut self, input: u8) {
        let mut b = input;
        if self.pending == Pending::Esc {
            self.pending = Pending::Nothing;
            if b == b'\\' {
                self.end_string(End::Dispatch);
                self.out.insert(Sent::Esc(Vec::new(), Some(b'\\')));
                self.clear();
                self.enter(Phase::Ground);
                return;
            }
            self.end_string(End::Cancel);
            self.clear();
            self.enter(Phase::Escape);
        }
        if matches!(b, 0x18 | 0x1a) {
            if self.phase.is_string() {
                self.end_string(End::Cancel);
            }
            self.out.insert(Sent::C0(b));
            self.clear();
            self.enter(Phase::Ground);
            return;
        }
        if b == 0x1b {
            if self.phase.is_string() {
                self.pending = Pending::Esc;
            } else {
                self.clear();
                self.enter(Phase::Escape);
            }
            return;
        }
        if b >= 0x80 {
            if self.enc == Enc::Latin1 && b <= 0x9f {
                self.c1(b);
                return;
            }
            if self.phase.is_string() {
                self.payload.push(b);
                return;
            }
            if self.enc == Enc::Utf8 || self.phase == Phase::Ground {
                if self.phase != Phase::Ground {
                    self.clear();
                    self.enter(Phase::Ground);
                }
                return;
            }
            b &= 0x7f;
        }
        match self.phase {
            Phase::Ground => {
                if b < 0x20 {
                    self.out.insert(Sent::C0(b));
                }
            }
            Phase::Escape | Phase::EscapeIntermediate => self.escape(b),
            Phase::CsiEntry | Phase::CsiParam | Phase::CsiIntermediate | Phase::CsiIgnore => {
                self.csi(b)
            }
            Phase::DcsEntry | Phase::DcsParam | Phase::DcsIntermediate => self.dcs(b),
            Phase::OscString => {
                if b == 0x07 {
                    self.end_string(End::Dispatch);
                    self.clear();
                    self.enter(Phase::Ground);
                } else if b >= 0x20 {
                    self.payload.push(b);
                }
            }
            Phase::SosPmApcString | Phase::DcsPassthrough | Phase::DcsIgnore => {
                self.payload.push(b)
            }
        }
    }

    fn escape(&mut self, b: u8) {
        if b < 0x20 {
            self.out.insert(Sent::C0(b));
            return;
        }
        if b == 0x7f {
            return;
        }
        if matches!(b, 0x20..=0x2f) {
            self.inters.push(b);
            self.enter(Phase::EscapeIntermediate);
            return;
        }
        if self.phase == Phase::Escape {
            let next = match b {
                b'[' => Some((Phase::CsiEntry, None)),
                b']' => Some((Phase::OscString, Some(Strand::Osc))),
                b'P' => Some((Phase::DcsEntry, None)),
                b'X' | b'^' => Some((Phase::SosPmApcString, Some(Strand::Other))),
                b'_' => Some((Phase::SosPmApcString, Some(Strand::Apc))),
                _ => None,
            };
            if let Some((p, s)) = next {
                self.clear();
                self.enter(p);
                if let Some(s) = s {
                    self.start(s);
                }
                return;
            }
        }
        self.esc_dispatch(b);
        self.clear();
        self.enter(Phase::Ground);
    }

    fn csi(&mut self, b: u8) {
        if b < 0x20 {
            self.out.insert(Sent::C0(b));
            return;
        }
        if b == 0x7f {
            return;
        }
        if self.phase == Phase::CsiIgnore {
            if matches!(b, 0x40..=0x7e) {
                self.clear();
                self.enter(Phase::Ground);
            }
            return;
        }
        if matches!(b, 0x40..=0x7e) {
            self.csi_dispatch(b);
            self.clear();
            self.enter(Phase::Ground);
            return;
        }
        if matches!(b, 0x20..=0x2f) {
            self.inters.push(b);
            self.enter(Phase::CsiIntermediate);
            return;
        }
        if self.phase == Phase::CsiIntermediate {
            self.enter(Phase::CsiIgnore);
            return;
        }
        if matches!(b, 0x3c..=0x3f) {
            if self.phase == Phase::CsiEntry {
                self.marker = Some(b);
                self.enter(Phase::CsiParam);
            } else {
                self.enter(Phase::CsiIgnore);
            }
            return;
        }
        self.params.push(b);
        self.enter(Phase::CsiParam);
    }

    fn dcs(&mut self, b: u8) {
        if b < 0x20 || b == 0x7f {
            return;
        }
        if matches!(b, 0x40..=0x7e) {
            let form = Form::of(self.marker, self.inters.last().copied(), b);
            self.out.insert(Sent::Dcs(form));
            self.enter(Phase::DcsPassthrough);
            self.start(Strand::Other);
            return;
        }
        if matches!(b, 0x20..=0x2f) {
            self.inters.push(b);
            self.enter(Phase::DcsIntermediate);
            return;
        }
        if self.phase == Phase::DcsIntermediate || b == b':' {
            self.enter(Phase::DcsIgnore);
            self.start(Strand::Other);
            return;
        }
        if matches!(b, 0x3c..=0x3f) {
            if self.phase == Phase::DcsEntry {
                self.marker = Some(b);
                self.enter(Phase::DcsParam);
            } else {
                self.enter(Phase::DcsIgnore);
                self.start(Strand::Other);
            }
            return;
        }
        self.params.push(b);
        self.enter(Phase::DcsParam);
    }
}

/// A rendition's parameters: each number, a colon's first sub-parameter,
/// and the selector after `38`, `48` or `58`, whose operands are skipped.
fn sgr_sent(fields: &[&str], out: &mut BTreeSet<Sent>) {
    if fields.is_empty() {
        out.insert(Sent::Sgr(0, SgrSub::Bare));
        return;
    }
    let mut i = 0;
    while i < fields.len() {
        let mut parts = fields[i].split(':');
        let n = saturating(parts.next().unwrap_or(""));
        out.insert(Sent::Sgr(n, SgrSub::Bare));
        if let Some(sub) = parts.next() {
            if !sub.is_empty() {
                out.insert(Sent::Sgr(n, SgrSub::Colon(saturating(sub))));
            }
            i += 1;
            continue;
        }
        if matches!(n, 38 | 48 | 58) && i + 1 < fields.len() {
            let s = saturating(head(fields[i + 1]));
            out.insert(Sent::Sgr(n, SgrSub::Semi(s)));
            i += match s {
                5 => 3,
                2 => 5,
                _ => 2,
            };
            continue;
        }
        i += 1;
    }
}

/// An operating system command's code, and the word after it up to `;` or
/// `=`.
fn osc_sent(payload: &[u8], out: &mut BTreeSet<Sent>) {
    let text = String::from_utf8_lossy(payload);
    let (code, rest) = match text.split_once(';') {
        Some((c, r)) => (c, Some(r)),
        None => (&*text, None),
    };
    if code.is_empty() || !code.bytes().all(|c| c.is_ascii_digit()) {
        return;
    }
    let Ok(code) = code.parse::<u32>() else {
        return;
    };
    out.insert(Sent::Osc(code, None));
    if let Some(rest) = rest {
        let word = rest.split(';').next().unwrap_or("");
        let word = word.split('=').next().unwrap_or("");
        if !word.is_empty() {
            out.insert(Sent::Osc(code, Some(word.to_string())));
        }
    }
}

/// A graphics command's control keys, `G` then `k=v,...` up to `;`.
fn apc_sent(payload: &[u8], out: &mut BTreeSet<Sent>) {
    let Some(rest) = payload.strip_prefix(b"G") else {
        return;
    };
    let control = rest.split(|&c| c == b';').next().unwrap_or(&[]);
    for item in control.split(|&c| c == b',') {
        let (k, v) = match item.iter().position(|&c| c == b'=') {
            Some(at) => (&item[..at], Some(&item[at + 1..])),
            None => (item, None),
        };
        if let [k] = k {
            out.insert(Sent::Apc(*k, None));
            if let Some(&[first, ..]) = v {
                out.insert(Sent::Apc(*k, Some(first)));
            }
        }
    }
}

/// Everything the atoms `keep` admits can send, each read from power-on.
fn emitted(keep: impl Fn(&Atom) -> bool) -> BTreeSet<Sent> {
    ALPHABET
        .iter()
        .filter(|a| keep(a))
        .flat_map(|a| lex(a.bytes))
        .collect()
}

/// A done row: the first cell of its table's header, and its own first cell.
struct DoneRow {
    table: String,
    first: String,
}

/// The cells of a table line, a `\|` read as a `|` inside a cell.
fn cells(line: &str) -> Vec<String> {
    let s = line.trim();
    let s = s.strip_prefix('|').unwrap_or(s);
    let s = if s.ends_with("\\|") {
        s
    } else {
        s.strip_suffix('|').unwrap_or(s)
    };
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' && it.peek() == Some(&'|') {
            it.next();
            cur.push('|');
        } else if c == '|' {
            out.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(c);
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Every done row, each with the table it is in. A row is done when any of
/// its cells' first word is `done`; the legend row, which defines
/// `**done**`, is not a row about an operation.
fn done_rows(doc: &str) -> Vec<DoneRow> {
    let mut rows = Vec::new();
    let mut table: Option<String> = None;
    for line in doc.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            table = None;
            continue;
        }
        let cs = cells(t);
        let Some(name) = table.clone() else {
            table = Some(cs[0].clone());
            continue;
        };
        if t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) || cs[0].contains("**done**") {
            continue;
        }
        if cs
            .iter()
            .any(|c| c.split_whitespace().next() == Some("done"))
        {
            rows.push(DoneRow {
                table: name,
                first: cs[0].clone(),
            });
        }
    }
    rows
}

/// The done rows `coverage.rs` counts, by its own reading of a line.
fn done_count(doc: &str) -> usize {
    doc.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('|'))
        .filter(|l| {
            let cs: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
            cs.len() >= 2
                && !cs[0].contains("**done**")
                && cs
                    .iter()
                    .any(|c| c.split_whitespace().next() == Some("done"))
        })
        .count()
}

/// A cell's code spans and the text between them.
enum Spanned {
    Code(String),
    Text(String),
}

/// The code spans of a cell, delimited by equal runs of backticks; an
/// unclosed span runs to the end of the cell.
fn spans(cell: &str) -> Vec<Spanned> {
    let c: Vec<char> = cell.chars().collect();
    let run = |from: usize| c[from..].iter().take_while(|&&x| x == '`').count();
    let mut out = Vec::new();
    let mut txt = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '`' {
            txt.push(c[i]);
            i += 1;
            continue;
        }
        let n = run(i);
        let open = i + n;
        let mut close = c.len();
        let mut k = open;
        while k < c.len() {
            if c[k] == '`' {
                let m = run(k);
                if m == n {
                    close = k;
                    break;
                }
                k += m;
            } else {
                k += 1;
            }
        }
        if !txt.is_empty() {
            out.push(Spanned::Text(std::mem::take(&mut txt)));
        }
        let body: String = c[open..close].iter().collect();
        let trimmed = n > 1 && body.len() > 2 && body.starts_with(' ') && body.ends_with(' ');
        out.push(Spanned::Code(if trimmed {
            body[1..body.len() - 1].to_string()
        } else {
            body
        }));
        i = close + n;
    }
    if !txt.is_empty() {
        out.push(Spanned::Text(txt));
    }
    out
}

/// Two hexadecimal digits.
fn hex_byte(t: &str) -> Option<u8> {
    if t.len() != 2 || !t.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u8::from_str_radix(t, 16).ok()
}

/// A token of a spelling as the byte it names: `SP`, or one character.
fn spelled_byte(t: &str) -> Option<u8> {
    match t.as_bytes() {
        b"SP" => Some(b' '),
        [b] => Some(*b),
        _ => None,
    }
}

/// `ESC` and what follows: intermediates first, then one final, or several
/// finals after an intermediate (`ESC ( C 5 H`), or none (`ESC (`).
fn esc_row(toks: &[&str]) -> Option<Vec<Sent>> {
    let bs: Vec<u8> = toks
        .iter()
        .copied()
        .map(spelled_byte)
        .collect::<Option<_>>()?;
    let inters: Vec<u8> = bs
        .iter()
        .copied()
        .take_while(|b| matches!(b, 0x20..=0x2f))
        .collect();
    let rest = &bs[inters.len()..];
    if rest.iter().any(|b| matches!(b, 0x20..=0x2f)) {
        return None;
    }
    match rest {
        [] if inters.is_empty() => None,
        [] => Some(vec![Sent::Esc(inters, None)]),
        [_, _, ..] if inters.is_empty() => None,
        _ => Some(
            rest.iter()
                .map(|&f| Sent::Esc(inters.clone(), Some(f)))
                .collect(),
        ),
    }
}

/// `CSI` and what follows: a mode set or reset, or a form with the
/// parameter count and first value the spelling gives.
fn csi_row(seq: &str) -> Option<Vec<Sent>> {
    let toks: Vec<&str> = seq.split_whitespace().skip(1).collect();
    let (&last, body) = toks.split_last()?;
    if matches!(last, "h" | "l" | "h/l") {
        let (marker, modes) = match body.split_first() {
            Some((&"?", rest)) => (Some(b'?'), rest),
            _ => (None, body),
        };
        if let [only] = modes {
            let ns: Vec<&str> = only.split('/').collect();
            if !ns
                .iter()
                .all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
            {
                return None;
            }
            return Some(
                ns.iter()
                    .map(|&n| Sent::Mode(marker, saturating(n)))
                    .collect(),
            );
        }
        if !modes.is_empty() || last == "h/l" {
            return None;
        }
    }
    let form = support::parse_form(seq)?;
    let params = match body.split_first() {
        Some((m, rest)) if matches!(*m, "?" | ">" | "<" | "=") => rest,
        _ => body,
    };
    let joined: String = params
        .iter()
        .filter(|t| **t != "SP" && !matches!(t.as_bytes(), [0x20..=0x2f]))
        .copied()
        .collect();
    let fields: Vec<&str> = if joined.is_empty() {
        Vec::new()
    } else {
        joined.split(';').collect()
    };
    if fields.iter().any(|f| f.contains("..")) {
        return Some(vec![Sent::Csi(form, None, None)]);
    }
    let first = fields
        .first()
        .copied()
        .filter(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()))
        .map(saturating);
    let arity = (fields.len() > 1).then_some(fields.len());
    Some(vec![Sent::Csi(form, arity, first)])
}

/// Words that stand for a value rather than spell one.
const PLACEHOLDERS: [&str; 8] = [
    "Pt", "Ps", "Pv", "metadata", "key", "prop", "payload", "...",
];

/// `OSC` and what follows: each code, and each first word the row spells.
fn osc_row(seq: &str) -> Option<Vec<Sent>> {
    let rest = seq[3..].trim();
    let (code, tail) = match rest.split_once(';') {
        Some((c, t)) => (c.trim(), Some(t)),
        None => (rest, None),
    };
    let codes: Vec<u32> = code
        .split('/')
        .map(|c| {
            if c.is_empty() || !c.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                c.parse().ok()
            }
        })
        .collect::<Option<_>>()?;
    let words: Vec<String> = tail
        .into_iter()
        .flat_map(|t| t.split(','))
        .map(|w| {
            let w = w.trim().split(';').next().unwrap_or("").trim();
            w.split('=').next().unwrap_or("").trim().to_string()
        })
        .filter(|w| !w.is_empty() && !PLACEHOLDERS.contains(&w.as_str()))
        .collect();
    let mut out = Vec::new();
    for &c in &codes {
        if words.is_empty() {
            out.push(Sent::Osc(c, None));
        }
        for w in &words {
            out.push(Sent::Osc(c, Some(w.clone())));
        }
    }
    Some(out)
}

/// `DCS` and what follows, up to its data: the form.
fn dcs_row(seq: &str) -> Option<Vec<Sent>> {
    let mut toks: Vec<&str> = seq.split_whitespace().skip(1).collect();
    while let Some(&t) = toks.last() {
        if !(matches!(t, "ST" | "Pt" | "..." | "Dscs") || t.starts_with("D...")) {
            break;
        }
        toks.pop();
    }
    let form = support::parse_form(&format!("CSI {}", toks.join(" ")))?;
    Some(vec![Sent::Dcs(form)])
}

/// A graphics command's `k=v` items: each key, with its value's first byte
/// where the row spells one.
fn apc_items(items: &str) -> Option<Vec<Sent>> {
    items
        .split(',')
        .map(|item| {
            let (k, v) = match item.split_once('=') {
                Some((k, v)) => (k, Some(v)),
                None => (item, None),
            };
            let [k] = k.as_bytes() else {
                return None;
            };
            if !k.is_ascii_alphabetic() {
                return None;
            }
            let v = v.and_then(|v| v.bytes().next()).filter(|&b| b != b'.');
            Some(Sent::Apc(*k, v))
        })
        .collect()
}

/// `APC G` and what follows.
fn apc_row(seq: &str) -> Option<Vec<Sent>> {
    apc_items(seq[3..].trim().strip_prefix("G ")?.trim())
}

/// One spelling: a byte (`0x07`, `8E`), a private mode (`?25`), or a
/// sequence.
fn seq_sent(seq: &str) -> Option<Vec<Sent>> {
    let s = seq.trim();
    let first = s.split_whitespace().next().unwrap_or("");
    if let Some(b) = first.strip_prefix("0x").and_then(hex_byte) {
        return Some(vec![Sent::C0(b)]);
    }
    if let Some(b) = hex_byte(s).filter(|b| matches!(b, 0x80..=0x9f)) {
        return Some(vec![Sent::C1(b)]);
    }
    if let Some(d) = s.strip_prefix('?') {
        if !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()) {
            return Some(vec![Sent::Mode(Some(b'?'), saturating(d))]);
        }
    }
    let toks: Vec<&str> = s.split_whitespace().collect();
    match toks.first().copied() {
        Some("ESC") => esc_row(&toks[1..]),
        Some("CSI") => csi_row(s),
        Some("OSC") => osc_row(s),
        Some("DCS") => dcs_row(s),
        Some("APC") => apc_row(s),
        _ => None,
    }
}

/// A span continuing the spelling before it: `l` after `CSI 15 h`, `" >`
/// after `ESC ( % 6`, `SetMark` after `OSC 1337 ; ...`, `a=T` after
/// `APC G a=t`.
fn relative(base: &str, s: &str) -> Option<String> {
    let s = s.trim();
    let bt: Vec<&str> = base.split_whitespace().collect();
    let is_inter = |t: &str| matches!(t.as_bytes().first(), Some(0x20..=0x2f));
    match bt.first().copied() {
        Some("ESC") => {
            let i = bt.get(1).copied().filter(|t| t.len() == 1 && is_inter(t))?;
            is_inter(s).then(|| format!("ESC {i} {s}"))
        }
        Some("CSI") if s.chars().count() == 1 => {
            let (stem, _) = base.rsplit_once(' ')?;
            Some(format!("{stem} {s}"))
        }
        Some("OSC") => {
            let code = base[3..].split(';').next().unwrap_or("").trim();
            (!s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic()))
                .then(|| format!("OSC {code} ; {s}"))
        }
        Some("APC") => Some(format!("APC G {s}")),
        _ => None,
    }
}

/// A range row, `CSI 1 t` to `CSI 10 t`: every first value between.
fn range_row(a: &str, b: &str) -> Option<Vec<Sent>> {
    match (seq_sent(a)?.as_slice(), seq_sent(b)?.as_slice()) {
        ([Sent::Csi(f, n, Some(lo))], [Sent::Csi(g, m, Some(hi))]) if f == g && n == m => {
            Some((*lo..=*hi).map(|v| Sent::Csi(*f, *n, Some(v))).collect())
        }
        _ => None,
    }
}

/// A row of a sequence table: its spellings, joined by `/`, `,` or spaces,
/// a range by `to`, up to the first other text.
fn seq_row(cell: &str) -> Option<Vec<Sent>> {
    let sp = spans(cell);
    if !matches!(sp.first(), Some(Spanned::Code(_))) {
        return None;
    }
    let mut keys = Vec::new();
    let mut base: Option<String> = None;
    let mut i = 0;
    while i < sp.len() {
        match &sp[i] {
            Spanned::Text(t) => {
                let sep = t.trim();
                if sep == "to" {
                    if let (Some(b), Some(Spanned::Code(next))) = (base.clone(), sp.get(i + 1)) {
                        keys.extend(range_row(&b, next)?);
                        base = Some(next.clone());
                        i += 2;
                        continue;
                    }
                }
                if !matches!(sep, "/" | "," | "") {
                    break;
                }
            }
            Spanned::Code(t) => {
                if let Some(k) = seq_sent(t) {
                    keys.extend(k);
                    base = Some(t.clone());
                } else {
                    let b = base.as_deref()?;
                    match relative(b, t).and_then(|r| seq_sent(&r)) {
                        Some(k) => keys.extend(k),
                        None => break,
                    }
                }
            }
        }
        i += 1;
    }
    (!keys.is_empty()).then_some(keys)
}

/// The numbers of a rendition row's text, each a run of digits and `:`,
/// `.`, `/`, `-` standing alone: `30-37`, `38/48`, `4:1..4:5`, `4:0`.
fn number_tokens(text: &str) -> Vec<String> {
    let c: Vec<char> = text.chars().filter(|&x| x != '*').collect();
    let in_run = |x: char| x.is_ascii_digit() || matches!(x, ':' | '.' | '/' | '-');
    let word = |x: char| x.is_alphanumeric() || x == '_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let starts = c[i].is_ascii_digit()
            && (i == 0 || !(word(c[i - 1]) || matches!(c[i - 1], ':' | '.' | '-')));
        if !starts {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < c.len() && in_run(c[j]) {
            j += 1;
        }
        let ends_alone = j == c.len() || !word(c[j]);
        let tok: String = c[i..j].iter().collect();
        let tok = tok.trim_end_matches(['.', '/']);
        if ends_alone && tok.ends_with(|x: char| x.is_ascii_digit()) {
            out.push(tok.to_string());
        }
        i = j;
    }
    out
}

/// A rendition number list: `30-37` or `38/48`.
fn number_list(tok: &str) -> Option<Vec<u16>> {
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    if let Some((a, b)) = tok.split_once('-') {
        if digits(a) && digits(b) {
            return Some((saturating(a)..=saturating(b)).collect());
        }
        return None;
    }
    let ns: Vec<&str> = tok.split('/').collect();
    ns.iter()
        .copied()
        .all(digits)
        .then(|| ns.iter().copied().map(saturating).collect())
}

/// A row of the rendition table: each number it names, a colon form
/// (`4:3`, `4:1..4:5`), and a code span's selector for the numbers before
/// it (`38/48 `;5;n``).
fn sgr_row(cell: &str) -> Option<Vec<Sent>> {
    let mut keys = Vec::new();
    let mut group: Vec<u16> = Vec::new();
    for sp in spans(cell) {
        match sp {
            Spanned::Code(t) => {
                let sep = t.chars().next()?;
                let digits: String = t.chars().skip(1).take_while(char::is_ascii_digit).collect();
                if digits.is_empty() || group.is_empty() || !matches!(sep, ';' | ':') {
                    return None;
                }
                let v = saturating(&digits);
                let sub = if sep == ';' {
                    SgrSub::Semi(v)
                } else {
                    SgrSub::Colon(v)
                };
                keys.extend(group.iter().map(|&n| Sent::Sgr(n, sub)));
            }
            Spanned::Text(t) => {
                for tok in number_tokens(&t) {
                    if let Some((n, subs)) = tok.split_once(':') {
                        let (lo, hi) = match subs.split_once("..") {
                            Some((lo, hi)) => (lo, hi.split(':').next_back().unwrap_or("")),
                            None => (subs, subs),
                        };
                        let n = saturating(n);
                        keys.extend(
                            (saturating(lo)..=saturating(hi))
                                .map(|v| Sent::Sgr(n, SgrSub::Colon(v))),
                        );
                        continue;
                    }
                    group = number_list(&tok)?;
                    keys.extend(group.iter().map(|&n| Sent::Sgr(n, SgrSub::Bare)));
                }
            }
        }
    }
    (!keys.is_empty()).then_some(keys)
}

/// A row of the state table: the state it names.
fn state_row(cell: &str) -> Option<Vec<Sent>> {
    match spans(cell).as_slice() {
        [Spanned::Code(name)] => Phase::ALL
            .iter()
            .find(|p| p.name() == name)
            .map(|&p| vec![Sent::State(p)]),
        _ => None,
    }
}

/// What a done row names, by its table's grammar; `None` for a row in
/// prose.
fn row_sent(row: &DoneRow) -> Option<Vec<Sent>> {
    match row.table.as_str() {
        "state" => state_row(&row.first),
        "parameters" => sgr_row(&row.first),
        "status" => None,
        _ => seq_row(&row.first),
    }
}

/// Why a done row in prose is not keyed.
enum Why {
    /// It names what the terminal sends, or a call an embedder makes: no
    /// byte the host sends reaches it.
    HostBound,
    /// It names a behaviour; these atoms send it.
    Atoms(&'static [&'static str]),
}

/// A done row and what it names: its keys, or `None` for prose.
struct Reading {
    row: DoneRow,
    keys: Option<Vec<Sent>>,
}

/// The done rows, each read.
fn read_rows(doc: &str) -> Vec<Reading> {
    done_rows(doc)
        .into_iter()
        .map(|row| {
            let keys = row_sent(&row);
            Reading { row, keys }
        })
        .collect()
}

/// Whether a prose row is named in [`EXEMPT`].
fn exempt(first: &str) -> bool {
    EXEMPT.iter().any(|e| e.row == first)
}

/// The rows the keys `sent` do not reach: a keyed row with a key not sent,
/// and a prose row [`EXEMPT`] does not name.
fn unreached(rows: &[Reading], sent: &BTreeSet<Sent>) -> BTreeSet<String> {
    rows.iter()
        .filter(|r| match &r.keys {
            Some(k) => !k.iter().all(|x| sent.contains(x)),
            None => !exempt(&r.row.first),
        })
        .map(|r| r.row.first.clone())
        .collect()
}

/// The prose rows neither [`EXEMPT`] nor [`UNREACHED`] names.
fn unnamed(rows: &[Reading]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.keys.is_none())
        .map(|r| r.row.first.clone())
        .filter(|f| !exempt(f) && !UNREACHED.contains(&f.as_str()))
        .collect()
}

/// `docs/vt-coverage.md`.
fn coverage_doc() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("docs/vt-coverage.md")
}

/// The list a failing check prints, ready to replace [`UNREACHED`] with.
fn as_rust(rows: &BTreeSet<String>) -> String {
    let mut s = String::from("const UNREACHED: &[&str] = &[\n");
    for r in rows {
        s += &format!("    {r:?},\n");
    }
    s + "];"
}

// Pins, not tests: every_done_row_is_mentioned_by_a_test skips from here.
/// A done row in prose the reach check does not key, and why.
struct Exemption {
    row: &'static str,
    why: Why,
}

/// The done rows in prose the reach check does not key.
const EXEMPT: &[Exemption] = &[
    Exemption {
        row: "UTF-8 decoding, including the `Utf8` parser state williams does not have",
        why: Why::Atoms(&["latin1x3", "wide-quad", "emoji-pair"]),
    },
    Exemption {
        row: "ill-formed UTF-8 — **one U+FFFD per maximal subpart**",
        why: Why::Atoms(&["fffd-after-prepend", "fffd-cut-after-prepend", "gr-bytes"]),
    },
    Exemption {
        row: "character width from East Asian Width + general category, generated from pinned Unicode data",
        why: Why::Atoms(&["narrow-quad", "wide-quad"]),
    },
    Exemption {
        row: "UAX #29 extended grapheme cluster segmentation",
        why: Why::Atoms(&["zwj-family", "flag", "devanagari-conjunct"]),
    },
    Exemption {
        row: "combining marks stored against their base cell",
        why: Why::Atoms(&["combining"]),
    },
    Exemption {
        row: "one cluster per cell — ZWJ sequences, flags and Indic conjuncts as a single cluster",
        why: Why::Atoms(&["zwj-family", "flag", "devanagari-conjunct"]),
    },
    Exemption {
        row: "`Emoji_Presentation=Yes` is two columns",
        why: Why::Atoms(&["emoji-pair"]),
    },
    Exemption {
        row: "ISO 8859-1 input, switched by `ESC % @` / `ESC % G`",
        why: Why::Atoms(&["latin1-enter", "utf8-enter"]),
    },
    Exemption {
        row: "encoding detection",
        why: Why::HostBound,
    },
    Exemption {
        row: "X10 mouse encoding (`ESC [ M Cb Cx Cy`)",
        why: Why::HostBound,
    },
];

/// The done rows the alphabet does not reach, by their first cell: a keyed
/// row with a key no atom sends, or a row in prose with no atom that sends
/// all it names.
const UNREACHED: &[&str] = &[
    "0 reset",
    "1, 2 bold/dim · 22 neither",
    "10-19 fonts",
    "20 Fraktur",
    "26, 50 proportional spacing",
    "3 italic · 23 not",
    "30-37, 39 foreground · 40-47, 49 background",
    "38/48 `:5:n` / `:2:Pi:r:g:b` sub-parameter form",
    "38/48 `;5;n` 256-colour, `;2;r;g;b` truecolour",
    "4 underline, 21 double, **4:1..4:5** styled · 24 / 4:0 not",
    "5, 6 blink · 25 steady",
    "51 framed, 53 overlined, 54, 55",
    "52 encircled",
    "58/59 underline colour",
    "60-63, 65 ideogram lines",
    "64 ideogram stress marking",
    "7 reverse · 27 positive",
    "7-bit `ESC Fe` forms `ESC P`, `ESC [`, `ESC \\`, `ESC ]`, `ESC X`, `ESC ^`, `ESC _`",
    "8 conceal · 28 reveal",
    "8-bit `84` IND, `85` NEL, `88` HTS, `8A` VTS, `8D` RI under `ESC % @`",
    "9 strike · 29 not",
    "90-97, 100-107 aixterm bright",
    "C0 in `csi_entry` — execute and **stay**, without consuming the marker position",
    "C0 in `csi_intermediate`",
    "C0 in `csi_param` — execute and **stay**, the parameters kept",
    "SUB additionally displaying the DEC error character `␦`",
    "`0x00` NUL",
    "`0x08` BS",
    "`0x0B` VT",
    "`0x0C` FF",
    "`0x0E` SO",
    "`0x0F` SI",
    "`18` CAN, `1A` SUB — cancel from any state, control strings included",
    "`1B` ESC — abandon and begin a new sequence",
    "`7F` DEL in `csi_entry` or `csi_intermediate`",
    "`7F` DEL in `csi_param`",
    "`7F` DEL in ground, ANSI and VT52",
    "`80-9F` C1 under `ESC % @`, inside a control string — cancel it, then execute, or enter `dcs_entry`, `csi_entry`, `osc_string` or `sos_pm_apc_string`; `9C` ends it as ST",
    "`80-9F` C1 under `ESC % @`, inside an escape or control sequence — cancel it, then execute, or enter `dcs_entry`, `csi_entry`, `osc_string` or `sos_pm_apc_string`; `9C` goes to ground",
    "`?1001`",
    "`?1010`",
    "`?1011`",
    "`?1014`",
    "`?1034`",
    "`?1035`",
    "`?1036`",
    "`?1037`",
    "`?1039`",
    "`?1041`",
    "`?1044`",
    "`?1045`",
    "`?1046`",
    "`?1050`",
    "`?1051`",
    "`?1052`",
    "`?1053`",
    "`?1060`",
    "`?1061`",
    "`?12`",
    "`?13`",
    "`?14`",
    "`?18`",
    "`?19`",
    "`?1`",
    "`?2001`, `?2005`, `?2006`",
    "`?2`",
    "`?30`",
    "`?35`",
    "`?36`",
    "`?41`",
    "`?42`",
    "`?43`",
    "`?44`",
    "`?45`",
    "`?46`",
    "`?4`",
    "`?5`",
    "`?66`",
    "`?67`",
    "`?8`",
    "`?96`",
    "`APC G a=d`",
    "`APC G a=f` / `a=a` / `a=c`, `d=f` / `d=F`",
    "`APC G a=p,P=...,Q=...`, `H`, `V`",
    "`APC G a=p,U=1` / `a=T,U=1`, `U+10EEEE`",
    "`APC G a=t` / `a=T` / `a=q`",
    "`APC G t=f` / `t=t` / `t=s`",
    "`CSI \" v`",
    "`CSI # P`, `CSI # Q`",
    "`CSI # R`",
    "`CSI # S`",
    "`CSI # {`, `CSI # }`",
    "`CSI # |`",
    "`CSI $ p`",
    "`CSI & u`",
    "`CSI ' }` / `CSI ' ~`",
    "`CSI 1 t` to `CSI 10 t`",
    "`CSI 11 t`, `CSI 13 t`, `CSI 13 ; 2 t`, `CSI 14 t`, `CSI 14 ; 2 t`, `CSI 15 t`, `CSI 16 t`, `CSI 18 t`, `CSI 19 t`",
    "`CSI 12 h` / `CSI 12 l`",
    "`CSI 14 h` / `CSI 14 l`",
    "`CSI 18 $ p`",
    "`CSI 2 ; Pu $ u`",
    "`CSI 2 h` / `CSI 2 l`",
    "`CSI 21 $ p`",
    "`CSI 22 ; Ps t`, `CSI 23 ; Ps t`",
    "`CSI > 0 q`",
    "`CSI > 2 p`",
    "`CSI > 4 ; 1 f`",
    "`CSI > 4 ; 2 m`",
    "`CSI > t`, `CSI > T`",
    "`CSI ? 1000/1002/1003 h/l`",
    "`CSI ? 1001 h/l`",
    "`CSI ? 1004 h/l`",
    "`CSI ? 1006 h/l`",
    "`CSI ? 1007 h/l`",
    "`CSI ? 1016 $ p` — DECRQM on the mouse modes, `?9`, `?1000`-`?1003`, `?1005`-`?1007`, `?1015`, `?1016`",
    "`CSI ? 1020 $ p` — DECRQM on xterm's `utf8` resource",
    "`CSI ? 1021 $ p` — DECRQM on xterm's `cjkWidth` resource",
    "`CSI ? 1022 $ p` — DECRQM on xterm's `emojiWidth` resource",
    "`CSI ? 1023 $ p` — DECRQM on xterm's `privateWidth` resource",
    "`CSI ? 1040 h/l`",
    "`CSI ? 1070 h/l`",
    "`CSI ? 1243 h` / `CSI ? 1243 l`",
    "`CSI ? 2 l`",
    "`CSI ? 2004 h/l`",
    "`CSI ? 2026 h/l`",
    "`CSI ? 2048 h/l`",
    "`CSI ? 4 m` / `CSI > 4 g`",
    "`CSI ? 5 W`",
    "`CSI ? 6 n` to `CSI ? 85 n`",
    "`CSI ? 80 h/l`",
    "`CSI ? 8452 h/l`",
    "`CSI ? 996 n`",
    "`CSI F`",
    "`CSI Pe ; Pb ; Pr ; Pc & w`",
    "`CSI Pl ; Pc \" p`",
    "`CSI Pm ' {`",
    "`CSI Pn SP P` / `CSI Pn SP Q` / `CSI Pn SP R`",
    "`CSI Pn SP b` / `CSI Pn SP a` / ``CSI Pn SP ` `` / `CSI Pn1 ; Pn2 SP c`",
    "`CSI Pn U` / `CSI Pn V`",
    "`CSI Pn Y`",
    "`CSI Ps # y`",
    "`CSI Ps $ u`",
    "`CSI Ps ' |`",
    "`CSI Ps ; Ps ; Ps ; Ps ; Ps T`",
    "`CSI Ps ; Pu ' z`",
    "`CSI Ps SP @` / `CSI Ps SP A`",
    "`CSI Ps W`",
    "`CSI Ps i` / `CSI ? Ps i`",
    "`CSI Ps t` with `Ps` 24 and up",
    "`CSI Ps1 ; Ps2 SP S`",
    "`CSI Pt ; Pl ; Pb ; Pr ' w`",
    "`CSI SP q`",
    "`CSI c`",
    "`CSI d`",
    "`CSI e`",
    "`CSI j`",
    "`CSI k`",
    "`CSI n`",
    "`CSI s` / `CSI u`",
    "`DCS + Q Pt ST`",
    "`DCS + q Pt ST`",
    "`DCS Pa ; Pb ; Ph q ... ST`",
    "`DCS Pc $ w ... ST`",
    "`ESC ( \" 4` `\" ?`",
    "`ESC ( % 6` `\" >` `% 2` `% =`",
    "`ESC ( <` / `ESC ( % 5`",
    "`ESC ( A`",
    "`ESC ( C 5 H 7 K Q 9 R f Y Z 4 = ` E 6`",
    "`ESC ( I`",
    "`ESC ( ` / `ESC ) ` / `ESC * ` / `ESC + `",
    "`ESC - ` / `ESC . ` / `ESC / `",
    "`ESC 7`",
    "`ESC =` / `ESC >`",
    "`ESC D`",
    "`ESC E`",
    "`ESC J`",
    "`ESC N` / `ESC O`, `8E` / `8F` under `ESC % @`",
    "`ESC SP F` — S7C1T, send 7-bit controls",
    "`ESC SP G` — S8C1T, send 8-bit controls",
    "`ESC SP L`, `ESC SP M`, `ESC SP N` — announce ISO 4873 Level 1, 2 or 3",
    "`ESC Z`",
    "`OSC 0/1/2`",
    "`OSC 104`",
    "`OSC 10` / `OSC 11`",
    "`OSC 110` / `OSC 111`",
    "`OSC 117` / `OSC 119`",
    "`OSC 12` / `OSC 112`",
    "`OSC 1337 ; Capabilities`",
    "`OSC 1337 ; ClearScrollback`",
    "`OSC 1337 ; Copy=:base64`",
    "`OSC 1337 ; CurrentDir=dir`, `OSC 1337 ; RemoteHost=user@host`",
    "`OSC 1337 ; CursorShape=N`",
    "`OSC 1337 ; HighlightCursorLine=yes",
    "`OSC 1337 ; ReportCellSize`",
    "`OSC 1337 ; ReportVariable=base64`",
    "`OSC 1337 ; RequestAttention=Pv`",
    "`OSC 1337 ; SetBadgeFormat=base64`",
    "`OSC 1337 ; SetColors=key=value`",
    "`OSC 1337 ; SetMark`",
    "`OSC 1337 ; SetUserVar=name=base64`",
    "`OSC 1337 ; UnicodeVersion=n`",
    "`OSC 21 ; key=value ; ...`",
    "`OSC 3 ; prop=value`",
    "`OSC 46 ; Pt`",
    "`OSC 4`",
    "`OSC 50 ; Pt`",
    "`OSC 5`, `OSC 105`, `OSC 6` / `OSC 106`",
    "`OSC 60`, `OSC 61 ; Pt`, `OSC 62 ; Pt`",
    "`OSC 7 ; Pt`",
    "`OSC 777 ; notify ; title ; body`",
    "`OSC 9 ; 4 ; st ; pr`",
    "`OSC 9 ; Pt`",
    "`OSC 99 ; metadata ; payload`",
    "`dcs_ignore`",
    "a byte `80-FF` inside a control sequence, in `csi entry`, `csi param`, `csi intermediate` and `csi ignore`, or after a VT52 `ESC` — **in UTF-8**",
    "the same introducers under `ESC % @` (ISO 8859-1)",
    "the same under `ESC % @` (ISO 8859-1)",
    "the walk: a colon group, an unknown parameter",
];
// every_done_row_is_mentioned_by_a_test skips to here.

/// Every done row of `docs/vt-coverage.md` is read, and every row the
/// alphabet does not reach is named in [`UNREACHED`], in both directions: a
/// row reached since is taken off the list, and a row that stops being
/// reached fails.
#[test]
fn every_done_row_is_reachable_from_the_generator() {
    let doc = coverage_doc();
    let rows = read_rows(&doc);
    assert_eq!(
        rows.len(),
        done_count(&doc),
        "a done row the reader did not read"
    );
    for e in EXEMPT {
        assert!(
            rows.iter()
                .any(|r| r.row.first == e.row && r.keys.is_none()),
            "EXEMPT names {}, which is not a done row in prose",
            e.row
        );
        if let Why::Atoms(names) = &e.why {
            for name in *names {
                atom(name);
            }
        }
    }
    let named = unnamed(&rows);
    assert!(
        named.is_empty(),
        "done rows in prose that nothing names: {named:#?}"
    );
    let gap = unreached(&rows, &emitted(|_| true));
    let pinned: BTreeSet<String> = UNREACHED.iter().map(|r| r.to_string()).collect();
    let newly: Vec<&String> = gap.difference(&pinned).collect();
    let since: Vec<&String> = pinned.difference(&gap).collect();
    assert!(
        newly.is_empty() && since.is_empty(),
        "not reached and not pinned: {newly:#?}\npinned and reached: {since:#?}\n{}",
        as_rust(&gap)
    );
}

/// A done row no atom sends is reported, and only that row.
#[test]
fn the_reach_check_fails_on_a_planted_row() {
    let doc = coverage_doc();
    let planted = format!(
        "{doc}\n| seq | name | status |\n|---|---|---|\n| `CSI ? 9999 h` | planted | done |\n"
    );
    let sent = emitted(|_| true);
    let before = unreached(&read_rows(&doc), &sent);
    let after = unreached(&read_rows(&planted), &sent);
    let extra: Vec<&String> = after.difference(&before).collect();
    assert_eq!(extra, ["`CSI ? 9999 h`"]);
}

/// Without the atom that alone sends `ESC M`, its row is reported, and
/// only its row.
#[test]
fn the_reach_check_fails_without_an_atom() {
    let rows = read_rows(&coverage_doc());
    let all = unreached(&rows, &emitted(|_| true));
    let without = unreached(&rows, &emitted(|a| a.name != "ri"));
    let extra: Vec<&String> = without.difference(&all).collect();
    assert_eq!(extra, ["`ESC M`"]);
}

/// One row of each table shape yields the keys written by hand, and a row
/// in prose that nothing names is reported.
#[test]
fn the_row_parser_reads_every_table() {
    let fixture = "\
| state | in baud | note |
|---|---|---|
| `csi_ignore` | done | |

| byte | name | status |
|---|---|---|
| `0x07` BEL | bell | done |

| seq | name | status |
|---|---|---|
| `ESC ( % 6` `\" >` | sets | done |
| `CSI Ps ; Ps ; Ps ; Ps ; Ps T` | five | done |
| `CSI 1 t` to `CSI 3 t` | range | done |
| `CSI 2 h` / `CSI ? 25 h/l`, `l` | modes | done |
| `CSI Pn $ \\|` | an escaped bar | done |
| `OSC 1337 ; File=args, FilePart`, `SetMark` | words | done |
| `DCS $ q Pt ST` | a string | done |
| `APC G a=p,P=...`, `H` | graphics | done |
| a row in prose | prose | done |

| mode | name | status |
|---|---|---|
| `?2001`, `?2005` | two | done |

| parameters | status |
|---|---|
| 4 underline, **4:1..4:2** styled · 24 / 4:0 not | done |
| 38/48 `;5;n` 256-colour | done |
";
    let want: Vec<Option<Vec<Sent>>> = vec![
        Some(vec![Sent::State(Phase::CsiIgnore)]),
        Some(vec![Sent::C0(0x07)]),
        Some(vec![
            Sent::Esc(b"(%".to_vec(), Some(b'6')),
            Sent::Esc(b"(\"".to_vec(), Some(b'>')),
        ]),
        Some(vec![Sent::Csi(Form::of(None, None, b'T'), Some(5), None)]),
        Some(
            [1, 1, 2, 3]
                .into_iter()
                .map(|v| Sent::Csi(Form::of(None, None, b't'), None, Some(v)))
                .collect(),
        ),
        Some(vec![
            Sent::Mode(None, 2),
            Sent::Mode(Some(b'?'), 25),
            Sent::Mode(Some(b'?'), 25),
        ]),
        Some(vec![Sent::Csi(
            Form::of(None, Some(b'$'), b'|'),
            None,
            None,
        )]),
        Some(vec![
            Sent::Osc(1337, Some("File".to_string())),
            Sent::Osc(1337, Some("FilePart".to_string())),
            Sent::Osc(1337, Some("SetMark".to_string())),
        ]),
        Some(vec![Sent::Dcs(Form::of(None, Some(b'$'), b'q'))]),
        Some(vec![
            Sent::Apc(b'a', Some(b'p')),
            Sent::Apc(b'P', None),
            Sent::Apc(b'H', None),
        ]),
        None,
        Some(vec![
            Sent::Mode(Some(b'?'), 2001),
            Sent::Mode(Some(b'?'), 2005),
        ]),
        Some(vec![
            Sent::Sgr(4, SgrSub::Bare),
            Sent::Sgr(4, SgrSub::Colon(1)),
            Sent::Sgr(4, SgrSub::Colon(2)),
            Sent::Sgr(24, SgrSub::Bare),
            Sent::Sgr(4, SgrSub::Colon(0)),
        ]),
        Some(vec![
            Sent::Sgr(38, SgrSub::Bare),
            Sent::Sgr(48, SgrSub::Bare),
            Sent::Sgr(38, SgrSub::Semi(5)),
            Sent::Sgr(48, SgrSub::Semi(5)),
        ]),
    ];
    let rows = read_rows(fixture);
    assert_eq!(rows.len(), done_count(fixture));
    let got: Vec<Option<Vec<Sent>>> = rows.iter().map(|r| r.keys.clone()).collect();
    assert_eq!(got, want);
    assert_eq!(unnamed(&rows), ["a row in prose"]);
}

/// The lexer keys a sequence by what was sent: an omitted parameter as its
/// default, a parameter count, a first value, a cancelled sequence as
/// nothing, and an 8-bit introducer only under ISO 8859-1.
#[test]
fn the_lexer_keys_what_it_lexes() {
    let has = |bytes: &[u8], want: &[Sent]| {
        let sent = lex(bytes);
        for w in want {
            assert!(sent.contains(w), "{bytes:?} does not send {w:?}: {sent:?}");
        }
    };
    let no_csi = |bytes: &[u8]| {
        let sent = lex(bytes);
        assert!(
            !sent.iter().any(|s| matches!(s, Sent::Csi(..))),
            "{bytes:?} sends a control sequence: {sent:?}"
        );
    };
    has(
        b"\x1b[1;;4m",
        &[
            Sent::Sgr(1, SgrSub::Bare),
            Sent::Sgr(0, SgrSub::Bare),
            Sent::Sgr(4, SgrSub::Bare),
        ],
    );
    has(
        b"\x1b[1;2;3;4;5T",
        &[Sent::Csi(Form::of(None, None, b'T'), Some(5), None)],
    );
    has(
        b"\x1b[18$p",
        &[Sent::Csi(Form::of(None, Some(b'$'), b'p'), None, Some(18))],
    );
    no_csi(b"\x1b[\x18");
    has(
        b"\x1b%@\x9b2J",
        &[Sent::Csi(Form::of(None, None, b'J'), None, Some(2))],
    );
    no_csi(b"\x9b2J");
    has(
        b"\x1b[38;5;1;4:3m",
        &[
            Sent::Sgr(38, SgrSub::Semi(5)),
            Sent::Sgr(4, SgrSub::Colon(3)),
        ],
    );
    has(
        b"\x1b]1337;SetMark\x07\x1b_Ga=p,i=1;\x1b\\",
        &[
            Sent::Osc(1337, Some("SetMark".to_string())),
            Sent::Apc(b'a', Some(b'p')),
            Sent::Esc(Vec::new(), Some(b'\\')),
        ],
    );
}
