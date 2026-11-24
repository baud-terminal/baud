//! **A live row carries its band's table**: a reader resolves a live
//! row's cells through the table its `RowStyles` holds, so it cannot
//! resolve them against another band's. `Term::styles` is every band's
//! table, read-only, and `Term::row_styles` a screen row's.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Back, RowStyles, StyleWord};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 4,
};

/// **On a terminal of one band, every live row resolves through that
/// band's table**: the screen's rows, by `row_styles` and by the view, and
/// a retained line, each carry the table `styles().one()` is, and a
/// written cell resolves to the rendition it was written in through each.
#[test]
fn a_live_row_resolves_through_the_one_bands_table() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[31mX\r\n\x1b[32mY\r\n\r\n\r\n");
        let v = t.styles();
        assert_eq!(v.bands().len(), 1, "one band");
        let one = v.one();
        assert!(v.bands().all(|b| b.len() == one.len()));
        // Row 0 scrolled off into history; Y is now on screen row 0.
        let y = t.row(Row::ZERO).expect("a row")[0];
        let RowStyles::Live(row) = t.row_styles(Row::ZERO) else {
            panic!("a screen row of the primary, up, is live")
        };
        assert_eq!(row.len(), one.len());
        assert_ne!(row.word(y.style), StyleWord::DEFAULT, "Y's rendition");
        assert_eq!(row.word(y.style), one.word(y.style));
        assert_eq!(t.style_in(t.row_styles(Row::ZERO), y), one.word(y.style));
        let RowStyles::Live(view) = t.viewport().styles(Row::ZERO) else {
            panic!("the view's screen row is live")
        };
        assert_eq!(view.word(y.style), one.word(y.style));
        let s = t.scrollback();
        let x = s.line(Back::of(0)).expect("X's line").0[0];
        assert_eq!(s.styles(Back::of(0)).word(x.style), one.word(x.style));
        assert_ne!(one.word(x.style), one.word(y.style), "two renditions");
    });
}

/// **Each band's table says which band it is, and has its own log**: on a
/// screen of 4-row bands, a rendition written on screen row 9 is in band
/// 2's table (`row_styles` says band 2), in band 2's log, which a renderer
/// resolves from (`band_styles_written`), and in no other band's, whose
/// logs hold their seeded default alone. Breaks: give every live table band
/// 0, or read the write path's band's log for every band.
#[test]
fn each_band_s_table_names_its_band_and_logs_its_own_writes() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 30,
    };
    let p = Provision::banded_in(&ws, BandRows::of(Rows::of_index(4)), BandSlots(1_024))
        .expect("40x30 is admitted banded");
    let mut owned = Owned::new(p);
    let mut t = Term::new(owned.storage(), &ws);
    // The pen back to the default before the cursor leaves band 2, so no
    // other band interns it.
    support::feed(&mut t, b"\x1b[10;1H\x1b[38;2;1;2;3mX\x1b[0m\x1b[H");
    let x = t.row(Row::from_index(9)).expect("a row")[0];
    let RowStyles::Live(row) = t.row_styles(Row::from_index(9)) else {
        panic!("a screen row is live")
    };
    let band = baud_domain::Band::of(2);
    assert_eq!(row.band(), band, "screen row 9 of 4-row bands");
    assert_ne!(row.word(x.style), StyleWord::DEFAULT);
    let written = |b: baud_domain::Band| -> Vec<baud_domain::StyleId> {
        match t.band_styles_written(b, baud_domain::StyleWrites::NEVER) {
            Written::Slots([a, b]) => a.iter().chain(b).copied().collect(),
            Written::Every => panic!("a fresh table has its log"),
        }
    };
    assert!(
        written(band).contains(&x.style),
        "band 2's log has X's slot"
    );
    for other in [0, 1, 3] {
        let b = baud_domain::Band::of(other);
        assert_eq!(
            written(b),
            [baud_domain::StyleId::DEFAULT],
            "band {other}'s log: its seeded default alone"
        );
    }
    assert!(t.band_style_writes(band) > t.band_style_writes(baud_domain::Band::of(1)));
    // Each band is its own table to a drawer, and a write leaves its serial.
    let serial = t.band_style_table(band);
    assert_ne!(serial, t.band_style_table(baud_domain::Band::of(1)));
    support::feed(&mut t, b"\x1b[10;2H\x1b[38;2;4;5;6mY\x1b[0m");
    assert_eq!(t.band_style_table(band), serial, "a write keeps the table");
    assert_eq!(t.band_style_reclaims(band).sweeps, 0, "nothing swept yet");
    assert_eq!(t.styles().table(band).band(), band);
}
