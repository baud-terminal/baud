//! Payload generators. These match the C harness byte for byte so the numbers
//! are comparable across the two implementations.
//!
//! Both are synthetic. Everything measured with them should be re-checked
//! against real capture (`script -c "vim ..."`) before it is designed around —
//! the style-reuse sweep showed the template cache's payoff moves with the
//! control-to-text ratio, which synthetic payloads set arbitrarily.

use baud_core::{Cols, Rows, WinSize};

/// The geometry the payloads are written against. Not two loose integers: the
/// same `WinSize` the terminal is configured from, so they cannot drift.
fn geometry() -> (Cols, Rows) {
    WinSize {
        ws_row: 50,
        ws_col: 200,
    }
    .geometry()
}

/// Pure CJK, no escapes at all: **every** byte goes through the UTF-8 decoder.
///
/// The synthetic payloads are ASCII, so `Term::utf8_share` reports 0.0% for
/// both and neither can price the decode path. This one exists to answer one
/// question -- what does baud cost per byte on multi-byte text? -- against
/// `streaming`, which answers it for ASCII. The gap between them is the decode
/// path's own cost, and it is what `[[source: utf8-lookup]]`'s 0.97
/// instructions per byte has to be compared against. Quoting the paper's 7.0
/// as baud's baseline was wrong: that is a standalone validator that runs its
/// state machine over ASCII too, which baud never does.
///
/// Three bytes per character and two columns per character, so a 200-column
/// row is 100 characters and 300 bytes. Deliberately without SGR: mixing in
/// escapes would measure the state machine again rather than the decoder.
pub fn cjk(cap: usize, _seq: &mut u64) -> Vec<u8> {
    // Common hanzi, all in U+4E00..U+9FFF -- three bytes in UTF-8, East Asian
    // Wide, so the width table and the cluster break both do real work.
    const W: [&str; 8] = [
        "終端", "文字", "解析", "画面", "行列", "描画", "入力", "出力",
    ];
    let (cols, _rows) = geometry();
    let mut b = Vec::with_capacity(cap);
    let mut col = 0usize;
    let mut i = 0usize;
    while b.len() < cap - 96 {
        if col > cols.index() - 8 {
            b.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        let w = W[i % W.len()];
        i += 1;
        b.extend_from_slice(w.as_bytes());
        // Two columns per CJK character.
        col += 2 * w.chars().count();
    }
    b
}

/// Bulk text: mostly printable, SGR roughly every 37 bytes, wraps near col 70.
/// Models `cat`, build output, log tailing.
pub fn streaming(cap: usize, seq: &mut u64) -> Vec<u8> {
    const W: [&str; 14] = [
        "static", "inline", "struct", "return", "const", "size_t", "uint32_t", "buffer", "render",
        "terminal", "parse", "handle", "width", "index",
    ];
    let mut b = Vec::with_capacity(cap);
    let mut col = 0usize;
    while b.len() < cap - 96 {
        if col > 70 {
            // CRLF, not a bare LF. The PTY here is in raw mode, so cfmakeraw has
            // disabled ONLCR and nothing translates it -- and LF is index only,
            // since LNM is reset by default. A bare LF would march the text
            // rightwards until it wrapped, which is not what a terminal receives.
            b.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        if b.len() % 37 == 0 {
            b.extend_from_slice(format!("\x1b[{}m", 30 + (b.len() % 8)).as_bytes());
            continue;
        }
        let s = W[((*seq as usize) + b.len() / 13) % 14].as_bytes();
        b.extend_from_slice(s);
        col += s.len();
        b.push(b' ');
        col += 1;
    }
    *seq += 1;
    b
}

/// Full-screen redraw: cursor-address to an arbitrary column, write, erase to
/// end of line, with periodic ECH / IL / ED. Models vim, tmux, htop.
///
/// The arbitrary column matters — addressing column 1 every time never forces a
/// mid-line write, which is exactly the case span storage is worst at. With
/// column 1 the run variant took 6,540 promotions; with arbitrary columns,
/// 19,709.
pub fn tui(cap: usize, seq: &mut u64) -> Vec<u8> {
    const W: [&str; 10] = [
        "NORMAL",
        "main.rs",
        "fn render",
        "  let mut buf",
        "impl Term",
        "    match ev",
        "// TODO",
        "pub struct",
        "0x1b5b",
        "-- INSERT --",
    ];
    let mut b = Vec::with_capacity(cap);
    while b.len() < cap - 160 {
        let s = *seq;
        let r = s % geometry().1.index() as u64 + 1;
        let c = 1 + s.wrapping_mul(37) % (geometry().0.index() as u64 - 80);
        b.extend_from_slice(format!("\x1b[{};{}H", r, c).as_bytes());
        if s.is_multiple_of(211) {
            b.extend_from_slice(format!("\x1b[{}m", 30 + s % 8).as_bytes());
        }
        for k in 0..6 {
            b.extend_from_slice(W[((s + k) % 10) as usize].as_bytes());
            b.push(b' ');
        }
        b.extend_from_slice(b"\x1b[K");
        if s.is_multiple_of(97) {
            b.extend_from_slice(b"\x1b[3X");
        }
        if s.is_multiple_of(401) {
            b.extend_from_slice(b"\x1b[2L");
        }
        if s.is_multiple_of(1601) {
            b.extend_from_slice(b"\x1b[J");
        }
        *seq += 1;
    }
    b
}
