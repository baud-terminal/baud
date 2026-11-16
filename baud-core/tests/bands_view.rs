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
