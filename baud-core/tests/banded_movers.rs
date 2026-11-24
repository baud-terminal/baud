//! **Every operation that moves rows keeps each cell's rendition across
//! bands**: a row copied into another band's row names the first band's
//! slots until it is translated (`Paint::moved`). Each mover runs on a
//! screen whose every cell has a rendition of its own, in bands of one row
//! and of four, and every line is read against a single table's.

use baud_core::*;
use baud_domain::{BandRepairs, BytePos, Content, ReadLen, Row, Rows, StyleWord};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

const BIG: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// Every cell of the screen: its content and the rendition its row's
/// table resolves.
fn screen(t: &Term<'_>) -> Vec<Vec<(Content, StyleWord)>> {
    let view = t.viewport();
    (0..view.rows().index())
        .map(|y| {
            let r = Row::from_index(y);
            let table = view.styles(r);
            view.row(r)
                .iter()
                .map(|c| (c.content, t.style_in(table, *c)))
                .collect()
        })
        .collect()
}

/// Every cell of the screen in a rendition of its own, the cursor home.
fn filled() -> Vec<u8> {
    let mut b = Vec::new();
    let mut i = 0usize;
    for r in 0..usize::from(WS.ws_row) {
        b.extend(format!("\x1b[{};1H", r + 1).as_bytes());
        for _ in 0..usize::from(WS.ws_col) {
            let n = i.to_le_bytes();
            b.extend(
                format!(
                    "\x1b[0;38;5;{};48;5;{}m{}",
                    n[0],
                    n[1],
                    char::from(b'a' + n[0] % 26)
                )
                .as_bytes(),
            );
            i += 1;
        }
    }
    b.extend(b"\x1b[0m\x1b[H");
    b
}

/// The movers: each a name and the bytes that move rows.
const MOVERS: [(&str, &[u8]); 8] = [
    ("SU in a region", b"\x1b[3;8r\x1b[2S\x1b[r"),
    ("SD in a region", b"\x1b[3;8r\x1b[2T\x1b[r"),
    ("IL", b"\x1b[4H\x1b[2L"),
    ("DL", b"\x1b[4H\x1b[2M"),
    ("LF at a region's bottom", b"\x1b[3;8r\x1b[8;1H\n\n\x1b[r"),
    (
        "RI at a region's top",
        b"\x1b[3;8r\x1b[3;1H\x1bM\x1bM\x1b[r",
    ),
    ("DECCRA across rows", b"\x1b[2;1;4;20;1;7;5;1$v"),
    ("full-screen SU", b"\x1b[3S"),
];

#[test]
fn every_mover_keeps_each_cell_s_rendition_across_bands() {
    for band in [1, 4] {
        for (name, bytes) in MOVERS {
            let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(band)), BandSlots(1_024))
                .expect("40x10 is admitted banded");
            let mut f_owned = Owned::new(p);
            let mut f = Term::new(f_owned.storage(), &WS);
            let mut b_owned = Owned::new(&BIG);
            let mut b = Term::new(b_owned.storage(), &WS);
            let fill = filled();
            for (t, s) in [(&mut f, &fill[..]), (&mut b, &fill[..])] {
                feed(t, s);
            }
            assert_eq!(screen(&f), screen(&b), "bands of {band}: the filled screen");
            feed(&mut f, bytes);
            feed(&mut b, bytes);
            assert_eq!(
                screen(&f),
                screen(&b),
                "bands of {band}, {name}: a cell drawn otherwise"
            );
            assert_eq!(
                f.band_repairs(),
                BandRepairs::default(),
                "bands of {band}, {name}: a pen repaired"
            );
        }
    }
}
