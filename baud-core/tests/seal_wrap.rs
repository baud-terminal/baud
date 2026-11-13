//! **The seal's wrap** (S3): at the last write generation a seal stamps every
//! row generation 0, draws a fresh epoch and starts again at 1. A reader
//! holding a seal from before the wrap must read every row as written --
//! the generations restart below its seal's, so a comparison of generations
//! alone would read every row clean forever after.
//!
//! Reached through `write-gen-probe`, which this crate's own
//! dev-dependency turns on: `Term::start_writes_at` sets the generation two
//! short of the wrap, and the real `Term::seal_writes` wraps.

#![cfg(feature = "write-gen-probe")]

mod support;

use baud_core::*;

#[test]
fn a_seal_kept_across_the_wrap_reads_the_rows_written_before_it() {
    let w = WinSize {
        ws_row: 10,
        ws_col: 20,
    };
    support::with_term(&w, |t| {
        support::feed(t, b"\x1b[1$~");
        t.start_writes_at(WriteGen::before_wrap(1));
        // A frame sealed, and its seal kept.
        let pre = t.seal_writes();
        // Row 3 written in the last frame before the wrap.
        support::feed(t, b"\x1b[4;1Hx");
        let g = t.row_gens(Rows::ZERO);
        assert!(g.after(Row::from_index(3), &pre), "written before the wrap");
        assert!(!g.after(Row::from_index(5), &pre), "the control");
        // The real wrap.
        let new = t.seal_writes();
        let g = t.row_gens(Rows::ZERO);
        assert!(
            g.after(Row::from_index(3), &pre),
            "row 3 against the kept seal"
        );
        assert!(t.wrote_since(&pre));
        assert!(t.status_gen().expect("a status line").after(&pre));
        // Under the new seal nothing is written until something is.
        assert!((0..10).all(|y| !g.after(Row::from_index(y), &new)));
        assert!(!t.wrote_since(&new));
        assert!(!t.status_gen().expect("a status line").after(&new));
        support::feed(t, b"\x1b[6;1Hy");
        assert!(t.row_gens(Rows::ZERO).after(Row::from_index(5), &new));
        assert!(t.wrote_since(&new));
    });
}
