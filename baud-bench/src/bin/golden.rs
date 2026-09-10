//! Capture what `baud-core2` renders, so its coverage outlives it.
//!
//! **This exists for one commit and is deleted with core2.**
//! `baud-bench/tests/differential.rs` drove both cores over the same bytes and
//! compared all 10,000 cells, and it is the instrument that found the `DECSTR`
//! alternate-screen bug, four marks bugs, HT, SO/SI, `ESC ( 0`, the
//! alternate-screen restore, `CAN`/`SUB` and the malformed-UTF-8 hang.
//!
//! What survives core2's deletion is a **golden** rendering: its answer,
//! recorded as data and asserted against `baud-core` from then on. That is
//! strictly weaker than a differential -- it cannot find a disagreement on
//! bytes nobody wrote a fixture for -- and much stronger than nothing, because
//! every fixture here was written *because* it found something.
//!
//! The recordings get a digest rather than a cell list: nine times ten thousand
//! cells is not a file anyone reads, and what a golden test has to do for them
//! is say "this changed", which the suites then explain.

use baud_core2::{OpMode, Storage, StreamByte, Term};
use std::io::Write;

const COLS: usize = 200;
const STRIDE: usize = 2048;

const TAB: &[u8] = b"a\tb\tc\r\n\x1b[3g\x1b[6GX\x1bH\x1b[1GA\tB\r\n\x1b[3gP\tQ";
const ALT: &[u8] = b"PRIMARY\r\nSECOND\x1b[?1049hALT\x1b[?1049l!";
const CHARSET: &[u8] =
    b"\x1b)0qqq\r\n\x0eqqq\r\n\x0fqqq\r\n\x1b(0qqq\r\n\x1b(Bqqq\r\n\x1b)0q\x0eq\x0fq\r\n";
const OSC: &[u8] = b"\x1b]0;title\x07AAA\r\n\x1b]0;title\x1b\\BBB\r\n\x1b]8;;http://example.com\x1b\\CCC\x1b]8;;\x1b\\DDD\r\n\x1b]0;a\nb\rc\x07EEE\r\n\x1b]0;t\x18FFF\x07GGG\r\n\x1b]0;t\x1aHHH\x07III";
const CANCEL: &[u8] = b"\x1b[1;5\x18~ab\r\n\x1b[1;5\x1a~cd\r\n\x1b\x18~ef";
const BADUTF8: &[u8] =
    b"\xe2Aab\r\n\xc2Bcd\r\n\xf0Cef\r\n\xe2\xe2\xe2gh\r\n\x80\x81\x82ij\r\n\xe2\x80Akl";

/// The wrap fixture, built the way the differential builds it.
fn wrap_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for extra in [1usize, 5, 199, 200, 201, 401] {
        bytes.extend(std::iter::repeat_n(b'x', COLS + extra));
        bytes.push(b'\n');
    }
    bytes.extend(std::iter::repeat_n(b'y', COLS));
    bytes.extend(b"tail\n");
    bytes.extend(std::iter::repeat_n(b'z', COLS - 1));
    bytes.extend("\u{4e16}\u{754c}".as_bytes());
    bytes.push(b'\n');
    bytes.extend(std::iter::repeat_n(b'w', COLS + 20));
    bytes.extend(b"\rover\n");
    bytes
}

/// The cursor-and-erase fixture, likewise.
fn cursor_bytes() -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(b"hello world this is a line of text to erase parts of\r\n");
    b.extend(b"\x1b[30;123Hplaced\x1b[K");
    b.extend(b"\x1b[1;1Htop\x1b[3;5Hindent\x1b[2K");
    b.extend(b"\x1b[10;10H\x1b[Jcleared below");
    b.extend(b"\x1b[5;5H\x1b[1Kstart-erased");
    b.extend(b"\x1b[7d\x1b[20Gcolumn\x1b[A\x1b[B\x1b[C\x1b[D");
    b
}

struct Shot {
    cells: Vec<(usize, usize, u32)>,
    cursor: (usize, usize),
    reply: Vec<u8>,
    digest: u64,
    nonblank: usize,
}

fn render(bytes: &[u8]) -> Shot {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut at = 0;
    while at < bytes.len() {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[at..]));
        let n = f.len();
        t.feed(base, n);
        t.commit_read(n);
        at += n.index();
    }
    let reply = t.reply().to_vec();
    let cursor = (t.cursor().0.index(), t.cursor().1.index());
    let vp = t.viewport();
    let mut cells = Vec::new();
    let mut digest = 0xcbf29ce484222325u64;
    let mut nonblank = 0usize;
    for y in 0..50 {
        for x in 0..COLS {
            let cell = vp[y * STRIDE + x];
            let v = match cell.content.as_codepoint() {
                Some(cp) => cp.value(),
                None if cell.content.is_empty() => u32::MAX,
                None => 0,
            };
            digest ^= u64::from(v);
            digest = digest.wrapping_mul(0x100000001b3);
            // Content and style, not the flag word: `CellFlags` has no public
            // accessor and a link or cluster index is an *index into a table*
            // whose numbering is each core's own, so comparing it across cores
            // would compare two arbitrary allocations.
            digest ^= cell.style.index() as u64;
            digest = digest.wrapping_mul(0x100000001b3);
            if v != u32::MAX {
                nonblank += 1;
                cells.push((y, x, v));
            }
        }
    }
    Shot {
        cells,
        cursor,
        reply,
        digest,
        nonblank,
    }
}

fn main() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-core/tests/golden");
    std::fs::create_dir_all(dir).unwrap();
    let wrap = wrap_bytes();
    let cursor = cursor_bytes();
    let mut rows: Vec<(&str, &[u8])> = vec![
        ("tab", TAB),
        ("alt", ALT),
        ("charset", CHARSET),
        ("osc", OSC),
        ("cancel", CANCEL),
        ("badutf8", BADUTF8),
        ("wrap", &wrap),
        ("cursor-erase", &cursor),
    ];
    rows.extend_from_slice(&[
        ("vt52-enter and leave", b"\x1b[?2lABC\x1b<DEF"),
        ("vt52-cursor up", b"\x1b[?2lline1\r\nline2\x1bAX"),
        ("vt52-cursor down", b"\x1b[?2lA\x1bBX"),
        ("vt52-cursor right", b"\x1b[?2lA\x1bCX"),
        ("vt52-cursor left", b"\x1b[?2lAB\x1bDX"),
        ("vt52-home", b"\x1b[?2labc\r\ndef\x1bHX"),
        ("vt52-reverse line feed", b"\x1b[?2l\r\n\r\nabc\x1bIX"),
        ("vt52-direct address", b"\x1b[?2l\x1bY\x28\x2aHERE"),
        ("vt52-address origin", b"\x1b[?2lxxxx\x1bY  TOP"),
        ("vt52-address low", b"\x1b[?2l\x1bY\x01\x01LOW"),
        ("vt52-address high", b"\x1b[?2l\x1bY\x7e\x7eHIGH"),
        (
            "vt52-erase to end of screen",
            b"\x1b[?2labc\r\ndef\r\nghi\x1bY\x21\x21\x1bJ",
        ),
        ("vt52-erase to end of line", b"\x1b[?2labcdef\x1bY  \x1bK"),
        ("vt52-graphics on", b"\x1b[?2l\x1bFlqk\x1bGabc"),
        ("vt52-csi is not a csi", b"\x1b[?2l\x1b[31mTEXT"),
        ("vt52-esc esc", b"\x1b[?2l\x1b\x1bAX"),
        ("vt52-can cancels", b"\x1b[?2l\x1b\x18AX"),
        ("vt52-sub cancels", b"\x1b[?2l\x1b\x1aBX"),
        (
            "vt52-print functions",
            b"\x1b[?2l\x1bW\x1bX\x1bV\x1b]\x1b^\x1b_ok",
        ),
        ("vt52-round trip", b"\x1b[?2lA\x1b<\x1b[1mB\x1b[?2lC\x1b<D"),
        (
            "vt52-csi works after leaving",
            b"\x1b[?2l\x1b<\x1b[31mSTYLED",
        ),
        ("decrqss-sgr default", b"\x1bP$qm\x1b\\"),
        ("decrqss-sgr bold", b"\x1b[1m\x1bP$qm\x1b\\"),
        ("decrqss-sgr many", b"\x1b[1;3;4;7;9m\x1bP$qm\x1b\\"),
        ("decrqss-sgr 256 fg", b"\x1b[38;5;200m\x1bP$qm\x1b\\"),
        (
            "decrqss-sgr truecolour",
            b"\x1b[38;2;12;34;56;48;2;65;43;21m\x1bP$qm\x1b\\",
        ),
        (
            "decrqss-sgr underline colour",
            b"\x1b[58;5;9m\x1bP$qm\x1b\\",
        ),
        ("decrqss-sgr font", b"\x1b[13m\x1bP$qm\x1b\\"),
        ("decrqss-stbm default", b"\x1bP$qr\x1b\\"),
        ("decrqss-stbm set", b"\x1b[5;40r\x1bP$qr\x1b\\"),
        ("decrqss-decsca off", b"\x1bP$q\"q\x1b\\"),
        ("decrqss-decsca on", b"\x1b[1\"q\x1bP$q\"q\x1b\\"),
        ("decrqss-decscusr default", b"\x1bP$q q\x1b\\"),
        ("decrqss-decscusr set", b"\x1b[5 q\x1bP$q q\x1b\\"),
        ("decrqss-decslrm default", b"\x1bP$qs\x1b\\"),
        ("decrqss-decslrm set", b"\x1b[?69h\x1b[5;10s\x1bP$qs\x1b\\"),
        ("decrqss-nonsense", b"\x1bP$qZZZ\x1b\\"),
        ("decrqss-empty target", b"\x1bP$q\x1b\\"),
        ("decrqss-sixel", b"\x1bPq#0;2;0;0;0#0~~@@vv@@~~@@~~$\x1b\\"),
        ("decrqss-decudk", b"\x1bP1;1|17/ab\x1b\\"),
        ("decrqss-bare dcs", b"\x1bPhello\x1b\\"),
        ("decrqss-bel terminated", b"\x1bP$qm\x07"),
        ("decrqss-cancelled by can", b"\x1bP$qm\x18\x1b\\"),
        ("decrqss-cancelled by sub", b"\x1bP$qm\x1a\x1b\\"),
        ("decrqss-two in a row", b"\x1bP$qm\x1b\\\x1bP$qr\x1b\\"),
        (
            "decrqss-non-query then query",
            b"\x1bPhello\x1b\\\x1bP$qm\x1b\\",
        ),
    ]);
    let mut f = std::fs::File::create(format!("{dir}/fixtures.txt")).unwrap();
    writeln!(f, "# core2's rendering of every differential fixture.").unwrap();
    writeln!(
        f,
        "# One `fixture` line, then `cursor`, `reply`, `digest`, then cells."
    )
    .unwrap();
    for (name, bytes) in &rows {
        let s = render(bytes);
        writeln!(f, "fixture {name}").unwrap();
        writeln!(
            f,
            "bytes {}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
        .unwrap();
        writeln!(f, "cursor {} {}", s.cursor.0, s.cursor.1).unwrap();
        writeln!(
            f,
            "reply {}",
            s.reply
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
        .unwrap();
        writeln!(f, "digest {:016x} nonblank {}", s.digest, s.nonblank).unwrap();
        for (y, x, cp) in &s.cells {
            writeln!(f, "cell {y} {x} {cp}").unwrap();
        }
        writeln!(f, "end").unwrap();
    }
    println!("{} fixtures captured", rows.len());

    // The nine recordings: a digest each, because nine times ten thousand
    // cells is not a file anyone reads.
    let mut g = std::fs::File::create(format!("{dir}/recordings.txt")).unwrap();
    writeln!(g, "# core2's rendering of each recording, as a digest.").unwrap();
    for rec in [
        "ascii/cat-scroll",
        "ascii/git-log",
        "ascii/less-pager",
        "ascii/asciiquarium",
        "utf8/btop",
        "utf8/vim-unicode",
        "utf8/cat-cjk",
        "utf8/less-cjk",
        "utf8/cava",
    ] {
        let p = format!(
            "{}/../tests/recordings/{rec}.raw",
            env!("CARGO_MANIFEST_DIR")
        );
        // **Not `else { continue }`.** The first run of this had
        // `asciiquarium` under `utf8/` where it lives in `ascii/`, and the
        // skip made a missing recording indistinguishable from a captured one
        // -- eight lines of output where nine were asked for, and nothing said
        // which.
        let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let s = render(&bytes);
        writeln!(
            g,
            "{rec} digest {:016x} nonblank {} cursor {} {}",
            s.digest, s.nonblank, s.cursor.0, s.cursor.1
        )
        .unwrap();
        println!("{rec}: digest {:016x}, {} non-blank", s.digest, s.nonblank);
    }
}
