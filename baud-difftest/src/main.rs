//! Differential test against a shipping terminal.
//!
//! Feeds identical bytes to `baud-core` and to alacritty's `Term`, then compares
//! the visible grid cell by cell.
//!
//! **It is the only external oracle left, and that is new.** Until `baud-core2`
//! was deleted the differential that mattered was between the two cores, and
//! this one was a second opinion; now it is the only thing in the repository
//! that can disagree with `baud-core` from outside it. `baud-core/tests/golden.
//! rs` froze what core2 rendered, but a golden capture only finds regressions on
//! bytes somebody wrote down -- this can find an operation nobody thought to
//! write a test for.
//!
//! **This file said `baud-core` and used `baud-core2` from the rename until the
//! deletion**, which is the same drift `docs.rs`'s `ROOTS` list and the CI step
//! carried: a name that kept reading correctly while pointing at the other
//! crate.
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;
use baud_core::*;
/// The geometry both sides are measured at, and the single place it is written.
///
/// `COLS`/`ROWS` are *derived* from it with `usize::from`, which is infallible
/// and is not a cast -- spelling 200x50 twice would let the two sides be
/// compared at different sizes, which is the one way this differential can lie
/// while looking green.
const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};
const COLS: usize = usize::from(WS.ws_col);
const ROWS: usize = usize::from(WS.ws_row);

#[derive(Clone)]
struct Noop;
impl EventListener for Noop {
    fn send_event(&self, _: Event) {}
}
#[derive(Clone, Copy)]
struct Dim;
impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize {
        ROWS
    }
    fn screen_lines(&self) -> usize {
        ROWS
    }
    fn columns(&self) -> usize {
        COLS
    }
}

// **`Owned::new` provisions exactly the geometry it is given**, because the
// grid is caller-provided here -- `baud-core2`'s `Storage` was a fixed
// `[PackedCell; GRID]` that could always grow, and this one cannot.
fn ours(bytes: &[u8]) -> Vec<char> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
    (0..ROWS)
        .flat_map(|y| {
            let r = t.row(Row::from_index(y)).to_vec();
            r.into_iter()
        })
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(if c.content.is_wide_tail() { '\0' } else { ' ' })
        })
        .collect()
}

fn theirs(bytes: &[u8]) -> Vec<char> {
    let mut cfg = Config::default();
    cfg.scrolling_history = 0;
    let mut t = ATerm::new(cfg, &Dim, Noop);
    let mut p = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
    p.advance(&mut t, bytes);
    let g = t.grid();
    (0..ROWS)
        .flat_map(|y| (0..COLS).map(move |x| (y, x)))
        .map(|(y, x)| {
            g[alacritty_terminal::index::Line(y as i32)][alacritty_terminal::index::Column(x)].c
        })
        .collect()
}

fn main() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("plain text", b"hello world".to_vec()),
        ("newline + cr", b"one\r\ntwo\r\nthree".to_vec()),
        ("SGR colour", b"\x1b[31mred\x1b[0m plain".to_vec()),
        ("cursor address", b"\x1b[5;10Hplaced".to_vec()),
        ("erase to EOL", b"abcdefgh\x1b[4G\x1b[K".to_vec()),
        ("erase screen", b"junk\x1b[2Jclean".to_vec()),
        ("erase chars", b"abcdefgh\x1b[3G\x1b[2X".to_vec()),
        ("insert lines", b"a\r\nb\r\nc\x1b[2;1H\x1b[1L".to_vec()),
        ("cursor motion", b"abc\x1b[2D\x1b[Bx".to_vec()),
        ("wrap at margin", vec![b'x'; COLS + 10]),
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
    ];
    let (mut pass, mut fail) = (0, 0);
    println!("  {:<20} {:>8}  {}", "case", "diff", "first mismatch");
    for (name, bytes) in &cases {
        let a = ours(bytes);
        let b = theirs(bytes);
        let diffs = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
        if diffs == 0 {
            pass += 1;
            println!("  {:<20} {:>8}  ok", name, 0);
        } else {
            fail += 1;
            let (i, (x, y)) = a
                .iter()
                .zip(b.iter())
                .enumerate()
                .find(|(_, (x, y))| x != y)
                .unwrap();
            println!(
                "  {:<20} {:>8}  cell {} (row {}, col {}): ours {:?} theirs {:?}",
                name,
                diffs,
                i,
                i / COLS,
                i % COLS,
                x,
                y
            );
        }
    }
    println!(
        "\n  {} match, {} differ, of {} cases",
        pass,
        fail,
        cases.len()
    );
}
