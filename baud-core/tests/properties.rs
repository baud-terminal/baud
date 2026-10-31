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
    // **A U+FFFD for ill-formed bytes after a Prepend joins the cluster**,
    // as a literal one does: a stray `80`, and a lead cut short at the end
    // of the atom, which the next atom's first byte ends and which a split
    // can fall inside.
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
/// swallowed, or a U+FFFD that took a cell, would do so in every chunking.
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
    fed("fffd-after-prepend", &|t| {
        assert_eq!(row(t), [u32::from(b'a'), u32::from(b'Z')]);
        let marks: Vec<u32> = t.marks(Row::ZERO, Col::ZERO).collect();
        assert_eq!(marks, [0x600, 0xfffd], "the U+FFFD joins the Prepend");
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
    let mut owned = Owned::new(&w).with_clipboard(CLIP);
    let mut t = Term::new(owned.storage(), &w);
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    f(&t);
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
        t.styles()[cell.style.index()]
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
