//! **A kept frame reads its set's copy where it lies**: a copy whose ring's
//! head is not 0 read through the overlay's view at its rotation, row `d`
//! at slot `(head + d) % ring`, cell for cell the screen's rows in order,
//! and no cell of it copied to be read.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, Row, RowExtent, RowRot, Rows, WinSize};
use baud_render::OverlayRows;
use baud_render::counters::{self, Counter};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 6,
};

/// The copy's ring: the window's rows and the row after them.
const RING: usize = 7;

/// The slot the ring's head is at.
const HEAD: usize = 4;

#[test]
fn a_ringed_copy_is_read_in_place_through_its_rotation() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = b"one\r\ntwo two\r\n\x1b[1mthree\x1b[m\r\nfour\x1b#6\r\nfive five five\r\nsix";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let screen = t.view(Rows::ZERO);
    let (cols, rows) = (screen.cols(), screen.rows());
    let stride = t.stride();
    let (s, ring) = (stride.index(), Rows::of_index(RING));
    let rot = RowRot::NONE.advance(HEAD, ring);
    // The copy as a frame's set holds it: row `d` at its slot.
    let mut copy = vec![PackedCell::default(); RING * s];
    let mut used = vec![RowExtent::default(); RING];
    for d in 0..rows.index() {
        let at = rot.advance(d, ring).index();
        let row = screen.row(Row::from_index(d));
        copy[at * s..at * s + row.len()].copy_from_slice(row);
        used[at] = screen.extent(Row::from_index(d));
    }
    let overlay = OverlayRows::default();
    let _ = counters::take();
    let view = overlay.view(&t, (&copy, &used), stride, (cols, rows), (ring, rot));
    for d in 0..rows.index() {
        let y = Row::from_index(d);
        assert_eq!(view.row(y), screen.row(y), "row {d}'s cells");
        assert_eq!(view.extent(y), screen.extent(y), "row {d}'s mark");
    }
    assert_eq!(view.cols(), Cols::of_index(WS.ws_col.into()));
    let copied = counters::take().of(Counter::CellsCopied);
    assert_eq!(copied.value(), 0, "cells copied to read the copy");
}

/// **The kept frame hands the view its copy at its rotation**: `draw_kept`
/// lays no copy of the set's rows out in screen order.
#[test]
fn the_kept_frame_lays_out_no_copy_of_the_set() {
    let src = include_str!("../../baud-app/src/app.rs");
    let at = src.find("fn draw_kept(").expect("draw_kept");
    let body = &src[at..];
    let body = &body[..body.find("\n    fn ").unwrap_or(body.len())];
    assert!(
        body.contains("overlay.view("),
        "draw_kept reads the copy through the view"
    );
    for copy in ["Vec<PackedCell>", "unrolled", ".collect()"] {
        assert!(!body.contains(copy), "draw_kept holds `{copy}`");
    }
    assert!(
        body.contains("(b.ring, b.rot)"),
        "draw_kept hands the view its rotation"
    );
}
