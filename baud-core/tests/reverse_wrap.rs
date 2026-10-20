//! `?45`, reverse-wraparound, as xterm's `CursorBack` defines it.
//!
//! ctlseqs says only "Reverse-wraparound mode (XTREVWRAP), xterm", and DEC
//! defines no such mode, so xterm's code is the definition (`[[source:
//! xterm-cursor-c]]` 121-191): `rev` is `REVERSEWRAP` with `WRAPAROUND`, so
//! the mode acts only under `?7`; a step back past the left margin goes to
//! the end of the line above only when that line is marked wrapped
//! (`LineTstWrapped`), and otherwise the cursor stops at the left margin;
//! and `CUB` takes the same walk as `BS`, `CASE_CUB` calling `CursorBack`
//! (`[[source: xterm-reset]]`'s file). Without `?1045` no top margin bounds
//! the walk, only an unwrapped line, unless `DECOM` is set: then the walk
//! stops at the top margin (`[[source: vt510-decom]]`).
mod support;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, ReadLen, Row, WinSize};

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// Twenty-five letters from the top left, so row 0 wraps into row 1.
const WRAPPED: &str = "\x1b[1;1Habcdefghijklmnopqrstuvwxy";

/// The cursor after `bytes`, fed at read sizes 1, 3 and whole.
fn cursor(bytes: &str) -> (usize, usize) {
    let b = bytes.as_bytes();
    let mut seen = Vec::new();
    for chunk in [1, 3, b.len()] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        for piece in b.chunks(chunk) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        let (r, c): (Row, Col) = t.cursor();
        seen.push((r.index(), c.index()));
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "{bytes:?}: {seen:?}");
    seen[0]
}

/// **Into a wrapped line, and not into an unwrapped one.** Row 0 wraps into
/// row 1, so `BS` from row 1's first column goes to row 0's last; row 2
/// does not continue row 1, so from row 2's first column it stays.
#[test]
fn backspace_reverse_wraps_only_into_a_wrapped_line() {
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\r\x08")), (0, 19));
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\x1b[3;1H\x08")), (2, 0));
    assert_eq!(cursor("\x1b[?45h\x1b[3;1H\x08"), (2, 0), "an empty screen");
}

/// **Only with `?7` set**: `rev` is `REVERSEWRAP` and `WRAPAROUND` both.
#[test]
fn reverse_wrap_needs_autowrap() {
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\x1b[?7l\r\x08")), (1, 0));
    assert_eq!(
        cursor(&format!("\x1b[?45h{WRAPPED}\x1b[?7l\r\x1b[D")),
        (1, 0)
    );
}

/// **`CUB` walks as `BS` does, its count included**: each step past the
/// left margin into a wrapped line goes to its end, and an unwrapped line
/// stops the walk at the left margin.
#[test]
fn cub_reverse_wraps_a_count_of_columns() {
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\r\x1b[D")), (0, 19));
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\r\x1b[3D")), (0, 17));
    assert_eq!(cursor(&format!("\x1b[?45h{WRAPPED}\r\x1b[65535D")), (0, 0));
    // Two wraps: row 1 into row 2 as well, then from row 2.
    let twice = "\x1b[1;1Habcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrs";
    assert_eq!(cursor(&format!("\x1b[?45h{twice}\r\x1b[25D")), (0, 15));
    // Without the mode `CUB` stops at the margin, as before.
    assert_eq!(cursor(&format!("{WRAPPED}\r\x1b[3D")), (1, 0));
}

/// **No top margin bounds it**: from the region's top a wrapped line above
/// it is reached, as xterm's `rev` tests the wrap and no margin.
#[test]
fn reverse_wrap_crosses_the_top_margin_into_a_wrapped_line() {
    assert_eq!(
        cursor(&format!("\x1b[?45h{WRAPPED}\x1b[2;6r\x1b[2;1H\x08")),
        (0, 19)
    );
    // And with nothing wrapped above it, it stays.
    assert_eq!(cursor("\x1b[?45h\x1b[2;6r\x1b[2;1H\x08"), (1, 0));
}

/// **Below the region, a reverse wrap that fails goes up a line**, xterm's
/// `if (row < bottom) ++row; /* reverse-wrap failed */` taken as written
/// (Jev's lean, XTERM_UP, 0.86/0.80 then 0.76/0.78): from row 7 under a
/// region ending at row 5 the line above is row 6, not before the bottom
/// margin, so the cursor is left on it. Inside the region it stays.
#[test]
fn a_failed_reverse_wrap_below_the_region_goes_up_a_line() {
    assert_eq!(cursor("\x1b[?45h\x1b[2;6r\x1b[8;1H\x08"), (6, 0));
    assert_eq!(cursor("\x1b[?45h\x1b[2;6r\x1b[7;1H\x08"), (5, 0));
    assert_eq!(cursor("\x1b[?45h\x1b[2;6r\x1b[5;1H\x08"), (4, 0));
}

/// The cursor and the `CPR` answer after `bytes`, fed whole.
fn cursor_and_cpr(bytes: &str) -> ((usize, usize), Vec<u8>) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, bytes.as_bytes());
    let (r, c): (Row, Col) = t.cursor();
    t.clear_reply();
    support::feed(&mut t, b"\x1b[6n");
    ((r.index(), c.index()), t.reply().to_vec())
}

/// **Under `DECOM` the walk stops at the top margin**, because origin mode
/// confines all cursor movement to the scrolling region (`[[source:
/// vt510-decom]]`). xterm's `rev` path reads no `ORIGIN` and would walk into
/// the wrapped line above the region, where `CPR`, which counts from the
/// region's top under `DECOM`, cannot name the cell; that is not followed.
/// Row 0 wraps into row 1 and the region is rows 1-5, so from its top left
/// `BS` and `CUB` stay, and `CPR` answers line 1, column 1.
#[test]
fn under_origin_mode_reverse_wrap_stops_at_the_top_margin() {
    let into = format!("{WRAPPED}\x1b[2;6r\x1b[?6h\x1b[?45h");
    for step in ["\x08", "\x1b[D", "\x1b[3D"] {
        let bytes = format!("{into}{step}");
        assert_eq!(cursor(&bytes), (1, 0), "{step:?}");
        assert_eq!(
            cursor_and_cpr(&bytes),
            ((1, 0), b"\x1b[1;1R".to_vec()),
            "{step:?}"
        );
    }
    // Inside the region a wrapped line is still walked into: row 1 wraps
    // into row 2, so from row 2's first column `BS` goes to row 1's end.
    let twice = "\x1b[1;1Habcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrs";
    let inside = format!("{twice}\x1b[2;6r\x1b[?6h\x1b[?45h\x1b[2;1H\x08");
    assert_eq!(cursor_and_cpr(&inside), ((1, 19), b"\x1b[1;20R".to_vec()));
}
