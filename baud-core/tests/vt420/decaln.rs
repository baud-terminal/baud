//! **`DECALN` puts origin mode, the rendition and every margin back**, then
//! fills and homes.
//!
//! `[[source: dec-std-070]]`'s DECALN (pp. D-19 and D-20, PDF 1206-1207,
//! read on the page images) runs, in order: `ORIGIN_MODE := ABSOLUTE`;
//! `CURRENT_RENDITION[BOLD]`, `[UNDERSCORE]`, `[BLINK]` and `[REVERSE]`
//! `:= FALSE`; every line single width and every cell filled; the Active
//! Position to line 1, column 1; `TOP_MARGIN := 1`,
//! `BOTTOM_MARGIN := MAX_NUM_LINES`; `LAST_COLUMN_FLAG := FALSE`. The
//! section is dated 18-Mar-1985 and names no side margins, which came with
//! the VT420; `[[source: vt510-decaln]]` says "DECALN sets the margins to the
//! extremes of the page, and moves the cursor to the home position", and the
//! margins are all four there. Neither text resets `DECLRMM`, the mode.
//!
//! Each test isolates one of the three, so the perturbation that removes it
//! turns that test red alone, and each is asserted whole, at every read size
//! and at every split into two reads. 20 by 8.

use baud_core::*;
use baud_domain::StyleWord;

const ROWS: usize = 8;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// Whether every cell on the screen draws the default rendition.
#[derive(Debug, PartialEq, Eq)]
enum Rendition {
    /// Every cell draws `StyleWord::DEFAULT`.
    Default,
    /// At least one cell draws another.
    Mixed,
}

/// What the bytes leave: the screen as strings (`.` blank), the rendition
/// its cells draw, and every reply in order.
#[derive(Debug, PartialEq, Eq)]
struct Seen {
    screen: Vec<String>,
    rendition: Rendition,
    reply: String,
}

fn seen_in(pieces: &[&[u8]]) -> Seen {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let mut reply = Vec::new();
    for piece in pieces {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        reply.extend_from_slice(t.reply());
        t.clear_reply();
    }
    let rows: Vec<&[PackedCell]> = (0..ROWS)
        .map(|y| &t.row(Row::from_index(y)).expect("a row on the grid")[..usize::from(WS.ws_col)])
        .collect();
    Seen {
        screen: rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .map_or('.', |cp| char::from_u32(cp.value()).unwrap_or('?'))
                    })
                    .collect()
            })
            .collect(),
        rendition: match rows
            .iter()
            .all(|r| r.iter().all(|c| t.style_of(*c) == StyleWord::DEFAULT))
        {
            true => Rendition::Default,
            false => Rendition::Mixed,
        },
        reply: String::from_utf8_lossy(&reply).into_owned(),
    }
}

/// The bytes whole, after requiring every read size and every split into two
/// reads to agree with them.
fn seen(bytes: &str) -> Seen {
    let b = bytes.as_bytes();
    let whole = seen_in(&[b]);
    for read in 1..b.len() {
        let pieces: Vec<&[u8]> = b.chunks(read).collect();
        assert_eq!(seen_in(&pieces), whole, "{read}-byte reads of {bytes:?}");
    }
    for cut in 1..b.len() {
        let (x, y) = b.split_at(cut);
        assert_eq!(seen_in(&[x, y]), whole, "split at {cut} of {bytes:?}");
    }
    whole
}

/// Where `ch` is, 0-based, if it is on the screen once.
fn find(s: &Seen, ch: char) -> Option<(usize, usize)> {
    let hits: Vec<(usize, usize)> = s
        .screen
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.chars()
                .enumerate()
                .filter(move |(_, c)| *c == ch)
                .map(move |(x, _)| (y, x))
        })
        .collect();
    match hits.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// **Origin mode goes back to absolute.** `DECRQM ?6` answers reset, and a
/// `DECSTBM 3;5` after it homes to line 1, column 1, "First column of first
/// line if Origin Mode is in the reset (Absolute) state" (STD 070 DECSTBM
/// note 1, p. 5-25), where with the mode left set it homed to the region's
/// top line. The margins are reset either way, so the home `DECALN` itself
/// makes cannot tell the mode, and this asks after it.
#[test]
fn decaln_resets_origin_mode() {
    let s = seen("\x1b[2;6r\x1b[?6h\x1b[3;3H\x1b#8\x1b[?6$p\x1b[3;5rX");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_str()),
        (Some((0, 0)), "\x1b[?6;2$y")
    );
}

/// **The side margins go to the extremes of the page, and `DECLRMM` stays.**
/// With origin mode never set, `CUP 1;9` is absolute either way; `ABC` then
/// runs on past column 10, where the old right margin would have wrapped the
/// `C` to the left margin of the next line. `DECRQM ?69` still answers set.
#[test]
fn decaln_resets_the_side_margins_and_keeps_the_mode() {
    let s = seen("\x1b[?69h\x1b[5;10s\x1b#8\x1b[1;9HABC\x1b[?69$p");
    assert_eq!(
        (find(&s, 'C'), s.reply.as_str()),
        (Some((0, 10)), "\x1b[?69;1$y")
    );
}

/// **The rendition goes back to normal before the fill**, so the `E`s and
/// what is written after them are plain. STD 070 resets the four renditions
/// its 1985 algorithm knows; the colours ride in the same Current Rendition
/// (STD 070 § 5.13.1, p. 5-186), and this core resets all of it, as `SGR 0`
/// would. Without the reset every cell carried bold red on green.
#[test]
fn decaln_resets_the_rendition_before_the_fill() {
    let s = seen("\x1b[1;4;5;7;31;42m\x1b#8X");
    assert_eq!(
        (find(&s, 'X'), s.rendition),
        (Some((0, 0)), Rendition::Default)
    );
}

/// **An `SGR` after it starts from the reset rendition.** The repeated-`SGR`
/// table answers a sequence it has seen from its bytes, as a masked replace
/// of the rendition word, and `DECALN` resets that word outside the `SGR`
/// path: the same `SGR 1;31` before and after must make the `X` bold red,
/// and an `SGR 31` seen before `DECALN` must not bring back the bold the
/// `SGR 1` before it set. Each is compared with the same `SGR` sent to a
/// terminal that never saw `DECALN`.
#[test]
fn an_sgr_after_it_starts_from_the_reset_rendition() {
    let drawn = |bytes: &[u8]| {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let x = t.row(Row::from_index(0)).expect("a row on the grid")[0];
        t.style_of(x)
    };
    let bold_red = drawn(b"\x1b[1;31mX");
    let red = drawn(b"\x1b[31mX");
    assert_ne!(bold_red, red, "the control: the two differ");
    assert_eq!(drawn(b"\x1b[1;31m\x1b#8\x1b[1;31mX"), bold_red);
    assert_eq!(drawn(b"\x1b[1m\x1b[31m\x1b#8\x1b[31mX"), red);
}
