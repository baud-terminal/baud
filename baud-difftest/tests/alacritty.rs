//! Differential test against a shipping terminal.
//!
//! Feeds identical bytes to `baud-core` and to alacritty's `Term`, then compares
//! the visible grid cell by cell.
//!
//! **A test target, not a binary**: `baud` is the one binary, and the rest is
//! part of the tests. What the binary printed and exited 1 on is
//! `the_report_lists_every_case_and_none_differs`; run it with
//! `cargo test --release -p baud-difftest -- --nocapture` to see the table.
//!
//! **It was the only external oracle left when `baud-core2` went.** Until
//! then the differential that mattered was between the two cores, and this one
//! was a second opinion; after it, this was the one thing in the repository
//! that could disagree with `baud-core` from outside it, until
//! `baud-sfte-difftest` added a second, against sfte, which compares the
//! cursor and the replies as well as the grid. `baud-core/tests/golden.rs`
//! froze what core2 rendered, but a golden capture only finds regressions on
//! bytes somebody wrote down -- a differential can find an operation nobody
//! thought to write a test for.
//!
//! **This file said `baud-core` and used `baud-core2` from the rename until the
//! deletion**, which is the same drift `docs.rs`'s `ROOTS` list and the CI step
//! carried: a name that kept reading correctly while pointing at the other
//! crate.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use baud_core::*;
/// The geometry both sides are measured at, and the single place it is written.
///
/// `cols()`/`rows()` are *derived* from it with `usize::from`, which is
/// infallible and is not a cast -- spelling 200x50 twice would let the two sides
/// be compared at different sizes, which is the one way this differential can
/// lie while looking green.
///
/// **They are functions rather than `const`s because `usize::from` is not const
/// on this toolchain**, and both const-compatible spellings are worse: a cast is
/// what the audit refuses, and a second literal is the transposition this comment
/// exists to prevent. Every call site is runtime, so `const` bought nothing here.
/// The first version of this file wrote them as `const`s and **did not compile at
/// all** -- which nothing noticed, because the manifest pinned alacritty by a
/// path that does not exist on this machine.
///
/// Do not spell the cast operator in a comment in this file: `cargo xtask audit`
/// matches the token in prose as well as in code, so describing one spends an
/// audit site. That is a detector flaw this repo already records, and it cost a
/// site here before this sentence replaced the one that named it.
const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};
fn cols() -> usize {
    usize::from(WS.ws_col)
}
fn rows() -> usize {
    usize::from(WS.ws_row)
}

#[derive(Clone)]
struct Noop;
impl EventListener for Noop {
    fn send_event(&self, _: Event) {}
}
#[derive(Clone, Copy)]
struct Dim;
impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize {
        rows()
    }
    fn screen_lines(&self) -> usize {
        rows()
    }
    fn columns(&self) -> usize {
        cols()
    }
}

// **`Owned::new` provisions exactly the geometry it is given**, because the
// grid is caller-provided here -- `baud-core2`'s `Storage` was a fixed
// `[PackedCell; GRID]` that could always grow, and this one cannot.
/// Whether a cell's background is the default or one an SGR set: a coloured
/// run that wraps, or an erase in a colour, must colour the same cells on
/// both sides, and a character-only comparison cannot see a coloured blank.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bg {
    Default,
    Set,
}

/// One cell as both sides are compared: what it shows, and whether its
/// background was set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cell {
    ch: char,
    bg: Bg,
}

fn ours(bytes: &[u8]) -> Vec<Cell> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
    let mut out = Vec::with_capacity(rows() * cols());
    for y in 0..rows() {
        for c in t.row(Row::from_index(y)).expect("a row on the grid") {
            let ch = t
                .base_of(*c)
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(if c.content.is_wide_tail() { '\0' } else { ' ' });
            let bg = if t.style_of(*c).bg().is_default() {
                Bg::Default
            } else {
                Bg::Set
            };
            out.push(Cell { ch, bg });
        }
    }
    out
}

/// How many cells one normalisation touched.
///
/// A newtype rather than a bare `usize` field, which is what the audit refused
/// when this struct first carried two of them -- and the refusal was right: the
/// two counters measure cells, and a primitive named `spacers` can be added to
/// anything. `baud_domain::Count` is the near neighbour and is not it: it is
/// `u16`-backed, it means "a repeat count in a control sequence", and it is not
/// re-exported by `baud-core`.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct Cells(usize);

impl Cells {
    const NONE: Cells = Cells(0);
    fn of(n: usize) -> Cells {
        Cells(n)
    }
    fn bump(&mut self) {
        self.0 += 1;
    }
    fn get(self) -> usize {
        self.0
    }
}

/// How many cells each normalisation below touched on one case.
///
/// **A normalisation that can fire without anyone noticing is a blind spot, not
/// a convenience.** Each of the two below exists to cancel a representation
/// difference that draws nothing on screen, and each is reported, so one that
/// starts firing on hundreds of cells says so instead of quietly agreeing.
struct Normalised {
    spacers: Cells,
    tabs: Cells,
}

impl Normalised {
    fn new() -> Normalised {
        Normalised {
            spacers: Cells::NONE,
            tabs: Cells::NONE,
        }
    }
}

/// alacritty's grid, as a renderer would draw it.
///
/// Two cells are **stored** differently by the two implementations and **drawn**
/// identically, and both are normalised here rather than pinned as differences.
/// Each normalisation is justified by alacritty's own source, not by assuming
/// the two agree:
///
/// * **The second half of a wide character.** baud stores `Content::WIDE_TAIL`,
///   a reserved value above `U+10FFFF`, deliberately -- so that "nothing written
///   here" cannot be confused with "a space was written", which differ for
///   selection, reflow and equality. alacritty stores a space and marks it
///   `Flags::WIDE_CHAR_SPACER`. The **flag** is what makes this an equivalence:
///   alacritty is saying the cell is a spacer, so it is read as one.
/// * **A horizontal tab.** `Term::put_tab` writes `'\t'` into the cell, and
///   only `if cell.c == ' '` -- so the byte never covers a glyph; it rides in a
///   blank so that selecting a tabbed region copies tabs. baud leaves the cells
///   untouched. On screen both are blank.
///
/// Anything else is compared as stored.
fn theirs(bytes: &[u8], n: &mut Normalised) -> Vec<Cell> {
    // No scrollback: the comparison is over the visible grid, and a retained
    // history would let alacritty answer from rows `rows()` cannot reach.
    let cfg = Config {
        scrolling_history: 0,
        ..Default::default()
    };
    let mut t = ATerm::new(cfg, &Dim, Noop);
    let mut p = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
    p.advance(&mut t, bytes);
    let g = t.grid();
    let mut out = Vec::with_capacity(rows() * cols());
    for y in 0..rows() {
        for x in 0..cols() {
            let cell =
                &g[alacritty_terminal::index::Line(y as i32)][alacritty_terminal::index::Column(x)];
            let wide_spacer = cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
            let ch = if wide_spacer {
                n.spacers.bump();
                '\0'
            } else if cell.c == '\t' {
                n.tabs.bump();
                ' '
            } else {
                cell.c
            };
            let bg = if cell.bg == Color::Named(NamedColor::Background) {
                Bg::Default
            } else {
                Bg::Set
            };
            out.push(Cell { ch, bg });
        }
    }
    out
}

/// The cases, in one place, so `main` and the test cannot drive different ones.
fn cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("plain text", b"hello world".to_vec()),
        ("newline + cr", b"one\r\ntwo\r\nthree".to_vec()),
        ("SGR colour", b"\x1b[31mred\x1b[0m plain".to_vec()),
        ("cursor address", b"\x1b[5;10Hplaced".to_vec()),
        ("erase to EOL", b"abcdefgh\x1b[4G\x1b[K".to_vec()),
        ("erase screen", b"junk\x1b[2Jclean".to_vec()),
        ("erase chars", b"abcdefgh\x1b[3G\x1b[2X".to_vec()),
        ("insert lines", b"a\r\nb\r\nc\x1b[2;1H\x1b[1L".to_vec()),
        ("cursor motion", b"abc\x1b[2D\x1b[Bx".to_vec()),
        ("wrap at margin", vec![b'x'; cols() + 10]),
        // A coloured run of blanks past the margin: the wrapped remainder is
        // coloured and the rest of its row is not, on both sides.
        ("wrapped background", {
            let mut v = b"\x1b[48;2;255;0;0m".to_vec();
            v.extend(vec![b' '; cols() + 44]);
            v.extend(b"\x1b[0m\r\nnext");
            v
        }),
        // Erase-line paints the whole row in the current background.
        (
            "erase-line bg",
            b"\x1b[48;2;0;0;255m\x1b[2K\x1b[0m\r\nafter".to_vec(),
        ),
        ("tab", b"a\tb\tc".to_vec()),
        ("backspace", b"abc\x08\x08X".to_vec()),
        ("UTF-8 CJK", "日本語".as_bytes().to_vec()),
        ("UTF-8 emoji", "ab🚀cd".as_bytes().to_vec()),
        ("combining mark", "cafe\u{301}".as_bytes().to_vec()),
        ("scroll region", b"\x1b[2;5r\x1b[3;1Hscrolled".to_vec()),
        ("delete chars", b"abcdefgh\x1b[3G\x1b[2P".to_vec()),
        ("insert blanks", b"abcdefgh\x1b[3G\x1b[2@".to_vec()),
        ("save/restore cur", b"\x1b7moved\x1b8back".to_vec()),
        ("reverse index", b"a\r\nb\x1bMx".to_vec()),
        // `CUU` and `CUD` stop at the top and bottom margins from their side
        // of them, DECOM reset ([[source: dec-std-070]] CUU and CUD, notes 1
        // and 2, pp. 5-41 and 5-43); `CUU` from below the region crosses it.
        (
            "CUU to the top margin",
            b"\x1b[5;20r\x1b[10;1H\x1b[99AX".to_vec(),
        ),
        (
            "CUU from below a region",
            b"\x1b[5;20r\x1b[30;1H\x1b[99AX".to_vec(),
        ),
        (
            "CUD to the bottom margin",
            b"\x1b[5;20r\x1b[10;1H\x1b[99BX".to_vec(),
        ),
        (
            "CNL to the bottom margin",
            b"\x1b[5;20r\x1b[10;3H\x1b[99EX".to_vec(),
        ),
        // A region that starts below the top and ends on the last row: an
        // LF at its bottom scrolls the region, and the header rows above it
        // stay where they are.
        (
            "region to last row",
            b"\x1b[1;1HH1\x1b[2;1HH2\x1b[5;50r\x1b[50;1HLAST\n\n\n".to_vec(),
        ),
        // An LF on the last row below a region neither scrolls nor moves.
        (
            "LF below region",
            b"\x1b[1;1HH1\x1b[1;49r\x1b[50;1HST\nXY".to_vec(),
        ),
        // A status line in a colour under a top region that scrolls sixty
        // times: the repair puts it back each time, to the edge, colour and
        // all, over rows that held something else.
        ("status under region", {
            let mut v = b"\x1b[50;1H\x1b[44mstatus\x1b[K\x1b[0m\x1b[1;49r\x1b[49;1H".to_vec();
            for i in 0..60 {
                v.extend(format!("line {i:02} of the log\n\r").as_bytes());
            }
            v
        }),
        // An `ESC` abandons the escape or control sequence in progress and
        // starts a new one ([[source: williams]]'s anywhere edge, which vte
        // follows): one case per node it abandons from.
        ("ESC in CSI params", b"\x1b[1\x1b[2mX".to_vec()),
        ("ESC in CSI, williams", b"\x1b[5;5H\x1b[3;1\x1b[HX".to_vec()),
        ("ESC after CSI", b"\x1b[\x1b[5;5HX".to_vec()),
        ("ESC in ignored CSI", b"\x1b[5;5H\x1b[1?\x1b[HX".to_vec()),
        ("ESC in escape", b"\x1b(\x1b0q".to_vec()),
        ("ESC after CSI inter", b"\x1b[ \x1b(0q".to_vec()),
        ("ESC after hash", b"\x1b7\x1b[5;5H\x1b#\x1b8X".to_vec()),
    ]
}

/// **alacritty's departures from the text, each pinned at its cell count.**
/// A pinned case must differ by exactly that many cells, so one alacritty
/// closes, or one that moves, comes back here.
///
/// * The four margin cases: with DECOM reset alacritty's `CUU`, `CUD` and
///   `CNL` go to the page's edge: each moves through `goto`, which bounds
///   the row by the region only under `ORIGIN`
///   (`[[source: alacritty-term-term-mod]]` 1156-1170, 1215-1230 and
///   1349-1354, the 0.26.0 crate's file byte for byte), where
///   `[[source: dec-std-070]]` CUU and CUD, notes 1 and 2 (pp. 5-41, 5-43),
///   stop them at the margin. The `X` lands on two rows: two cells.
const DEPARTS: &[(&str, usize)] = &[
    ("CUU to the top margin", 2),
    ("CUU from below a region", 2),
    ("CUD to the bottom margin", 2),
    ("CNL to the bottom margin", 2),
];

/// Off its pin: an unpinned case that differs, or a pinned one that differs
/// by another count, none included.
fn off_pin(v: &Verdict) -> bool {
    let pinned = DEPARTS
        .iter()
        .find(|(n, _)| *n == v.name)
        .map_or(0, |p| p.1);
    v.diffs.get() != pinned
}

/// One case's verdict: how many cells differ, and where the first one is.
struct Verdict {
    name: &'static str,
    diffs: Cells,
    first: Option<String>,
}

/// Drive both implementations over every case and compare.
///
/// Returns the verdicts and the normalisation counts, so the report can print
/// a table and the tests can assert on the same numbers rather than on a reparse
/// of the table.
fn compare() -> (Vec<Verdict>, Normalised) {
    let mut norm = Normalised::new();
    let verdicts = cases()
        .into_iter()
        .map(|(name, bytes)| {
            let a = ours(&bytes);
            let b = theirs(&bytes, &mut norm);
            let diffs = Cells::of(a.iter().zip(b.iter()).filter(|(x, y)| x != y).count());
            let first = a
                .iter()
                .zip(b.iter())
                .enumerate()
                .find(|(_, (x, y))| x != y)
                .map(|(i, (x, y))| {
                    format!(
                        "cell {} (row {}, col {}): ours {:?} theirs {:?}",
                        i,
                        i / cols(),
                        i % cols(),
                        x,
                        y
                    )
                });
            Verdict { name, diffs, first }
        })
        .collect();
    (verdicts, norm)
}

/// The table the binary printed, and its exit status as the test's verdict.
#[test]
fn the_report_lists_every_case_and_none_differs() {
    let (verdicts, norm) = compare();
    let fail = verdicts.iter().filter(|v| off_pin(v)).count();
    println!("  {:<20} {:>8}  first mismatch", "case", "diff");
    for v in &verdicts {
        match &v.first {
            None => println!("  {:<20} {:>8}  ok", v.name, 0),
            Some(w) if !off_pin(v) => {
                println!("  {:<20} {:>8}  pinned: {}", v.name, v.diffs.get(), w)
            }
            Some(w) => println!("  {:<20} {:>8}  {}", v.name, v.diffs.get(), w),
        }
    }
    println!(
        "\n  {} match or differ as pinned, {} off their pins, of {} cases",
        verdicts.len() - fail,
        fail,
        verdicts.len()
    );
    println!(
        "  normalised: {} wide-char spacers, {} tab cells",
        norm.spacers.get(),
        norm.tabs.get()
    );

    // **An oracle that only prints cannot fail.** This exited 0 whatever it
    // found, so even once it compiled it could not be a gate -- the same shape
    // as the glyph differential that ended in `let _ = want;` and passed green
    // while three recordings diverged by 3,057 cells.
    assert_eq!(fail, 0, "{fail} of {} cases off their pins", verdicts.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Being a workspace member makes `cargo test --workspace` *build* this
    /// crate; this test is what makes it *run*.** Compiling catches the rot that
    /// hid here for the life of the absolute path. It does not catch a cell
    /// divergence, because a binary with no `#[test]` is built and then not
    /// executed -- so the oracle would still only speak when somebody ran it by
    /// hand.
    #[test]
    fn every_case_agrees_with_alacritty() {
        let (verdicts, norm) = compare();
        let differing: Vec<String> = verdicts
            .iter()
            .filter(|v| off_pin(v))
            .map(|v| {
                format!(
                    "{}: {} cells, first {}",
                    v.name,
                    v.diffs.get(),
                    v.first.as_deref().unwrap_or("?")
                )
            })
            .collect();
        assert!(
            differing.is_empty(),
            "baud-core and alacritty render these off their pins:\n  {}",
            differing.join("\n  ")
        );

        // **The normalisations must stay small, or they are hiding something.**
        // Two cells of one recording's grid draw nothing in both
        // implementations and are stored differently; on this corpus that is 4
        // wide-char spacers and 2 tab cells, and the exact counts are pinned
        // rather than bounded so a normalisation that starts firing somewhere
        // new has to come and change this number.
        assert_eq!(norm.spacers.get(), 4, "wide-char spacers normalised");
        assert_eq!(norm.tabs.get(), 2, "tab cells normalised");
    }
}
