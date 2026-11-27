//! **A band holds its rows' renditions and [`BAND_RESERVED`] more, exactly**:
//! a band filled to its reserve takes the feed that needs every reserved
//! slot, and one slot fewer refuses it. No real geometry fills a band to its
//! reserve -- a table is a power of two and a pitch a whole number of cache
//! lines -- so the test holds the band's spare slots past the reserve
//! (`probe-band-pin`) and feeds the rest.
//!
//! The feed: every cell of band 0 in a rendition of its own, a pen no cell
//! names, and a rendition interned while that pen is current, written over a
//! cell that still names the one it replaces -- the default, the pen and the
//! rendition in flight, the three the reserve counts.
#![cfg(feature = "probe-band-pin")]

use baud_core::*;
use baud_domain::{Band, BandRepairs, BytePos, Content, ReadLen, Row, Rows, StyleWord};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 30,
};

const BIG: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};

const SLOTS: usize = 1_024;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// Every cell of the screen: its content and the rendition its row's table
/// resolves.
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

/// Band 0's rows, each cell in a rendition of its own; then a pen no cell
/// names; then a rendition interned while that pen is current, written over
/// the first cell.
fn the_feed(band_rows: usize) -> Vec<u8> {
    let mut b = Vec::new();
    let mut i = 0usize;
    for r in 0..band_rows {
        b.extend(format!("\x1b[{};1H", r + 1).as_bytes());
        for _ in 0..usize::from(WS.ws_col) {
            let n = i.to_le_bytes();
            b.extend(format!("\x1b[0;38;5;{};48;5;{}mx", n[0], n[1]).as_bytes());
            i += 1;
        }
    }
    b.extend(b"\x1b[0;38;2;1;2;3m\x1b[H\x1b[0;38;2;4;5;6mZ");
    b
}

/// Band 0 of a 40x30 terminal, bands asked 64 rows of 1,024 slots, with
/// `pins` of its spare slots held, fed: whether its screen reads as a
/// single table's does.
fn fed_with(pins: usize) -> (bool, BandSlots) {
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(64)), BandSlots(SLOTS))
        .expect("40x30 is admitted banded");
    let mut owned = Owned::new(p);
    let band_rows = owned.band_rows().rows().index();
    let mut t = Term::new(owned.storage(), &WS);
    let held = t.pin_band_slots(Band::FIRST, BandSlots(pins));
    let mut big = Owned::new(&BIG);
    let mut r = Term::new(big.storage(), &WS);
    let bytes = the_feed(band_rows);
    feed(&mut t, &bytes);
    feed(&mut r, &bytes);
    assert_eq!(t.band_repairs(), BandRepairs::default(), "no pen repaired");
    (screen(&t) == screen(&r), held)
}

#[test]
fn a_band_takes_its_reserve_and_refuses_past_it() {
    let p = Provision::banded_in(&WS, BandRows::of(Rows::of_index(64)), BandSlots(SLOTS))
        .expect("40x30 is admitted banded");
    let owned = Owned::new(p);
    let (stride, band_rows) = (owned.stride().index(), owned.band_rows().rows().index());
    // The controls: every cell of a band's rows can be written, the band
    // is band 0's rows on the screen, and the spare is what the test pins.
    assert_eq!(
        stride,
        usize::from(WS.ws_col),
        "a pitch of the screen's width"
    );
    assert!(band_rows <= usize::from(WS.ws_row), "band 0 on the screen");
    let spare = SLOTS - band_rows * stride;
    assert_eq!(
        BAND_RESERVED, 3,
        "the default, the pen, the rendition in flight"
    );
    assert!(spare >= BAND_RESERVED, "a provision admits the reserve");
    let fits = spare - BAND_RESERVED;
    let (same, held) = fed_with(fits);
    assert_eq!(held, BandSlots(fits), "every pin held");
    assert!(
        same,
        "a band at its reserve takes the feed ({spare} spare, {fits} held)"
    );
    let (same, held) = fed_with(fits + 1);
    assert_eq!(held, BandSlots(fits + 1), "every pin held");
    assert!(!same, "one slot past the reserve refuses the feed");
}
