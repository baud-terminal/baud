//! **A storage's footprint is its provision's arithmetic**: the cells of
//! the ring, the alternate screen and the status row, and a table of
//! `band_slots` slots for each band, each counted to the byte, so the
//! memory figures built on it cannot read 0 or drift from what is lent.

use baud_core::*;
use baud_domain::{BandRepairs, Depth, Reach, Rows, StyleId, StyleWord};

#[test]
fn a_footprint_is_the_provision_s_arithmetic() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 30,
    };
    let depth = Depth::of(100);
    let p = Provision::banded_in(&ws, BandRows::of(Rows::of_index(8)), BandSlots(1_024))
        .expect("40x30 is admitted banded");
    let owned = Owned::with_history(p, &CacheGeometry::new(None, None, None, None), depth);
    let f = owned.footprint();
    let stride = owned.stride().index();
    let band = owned.band_rows().rows().index();
    // A band each 8 rows of the 130-row ring and of the 30-row alternate
    // screen, and the status row's.
    let bands = (30 + 100usize).div_ceil(band) + 30usize.div_ceil(band) + 1;
    let slot = core::mem::size_of::<Option<StyleWord>>()
        + core::mem::size_of::<StyleId>()
        + core::mem::size_of::<Reach>();
    assert_eq!(
        f.tables.index(),
        bands * 1_024 * slot + bands * core::mem::size_of::<StyleBook>(),
        "a table, a log and a reach of 1,024 slots for each of {bands} bands, and a book each"
    );
    let cells = (130 + 30 + 1) * stride * core::mem::size_of::<PackedCell>();
    assert!(
        f.cells.index() > cells,
        "the ring, the alternate screen and the status row, with marks"
    );
    assert!(
        f.pool.index() > 0,
        "a banded storage lends a pool for a change of width"
    );
    assert_eq!(BandRepairs::default().pens, 0);
}
