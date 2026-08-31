//! **Nothing here grows, and nothing here allocates.**
//!
//! The claim this file turns into a test: every structure the ingest path
//! touches is a fixed size decided at compile time, and all of the memory is
//! the caller's. `baud-core` is `#![no_std]` with no `alloc`, so an allocation
//! would not compile -- but a *size* can drift silently, and `PackedCell` in
//! particular is eight bytes for measured reasons this repo records: four
//! could not hold a codepoint, and a ninth byte would round the cell to
//! twelve.
//!
//! Sizes are asserted rather than printed. A number nothing checks drifts, and
//! these ones decide how much traffic a screenful of text makes.

use baud_core::*;

/// **Eight bytes, and the reasons are measured.**
///
/// `Content(u32)` + `StyleId` + `CellFlags(u16)`. Four bytes could not hold a
/// codepoint -- `U+1F680 & 0xffff` is `U+F680` -- and the same eight bytes
/// spelled `{u32,u16,u8,u8}` cost 4% because LLVM stopped merging the store.
#[test]
fn a_cell_is_eight_bytes() {
    assert_eq!(core::mem::size_of::<PackedCell>(), 8);
    assert_eq!(core::mem::align_of::<PackedCell>(), 4);
}

/// The grid is a fixed number of fixed cells, and the caller owns it.
///
/// `GRID` is the ceiling a caller must provide for the largest screen; the
/// live grid strides by the column count, which is what keeps consecutive rows
/// off the same L1 sets.
#[test]
fn the_grid_is_a_fixed_number_of_fixed_cells() {
    assert_eq!(GRID, MAX_ROWS * MAX_COLS);
    assert_eq!(GRID * core::mem::size_of::<PackedCell>(), 640 * 2048 * 8);
}

/// **Every table is sized at compile time.** These are the numbers rule #2 --
/// working set stays resident -- is actually about.
#[test]
fn every_table_has_a_fixed_size() {
    // The three-byte cell table: one entry per `(lead, cont, cont)`.
    assert_eq!(CELLS3, CELLS3_PREFIXES * 256);
    assert_eq!(CELLS3_PREFIXES, 16 * 64);
    // The rendition table, bounded so it cannot walk out of cache under
    // truecolour key pressure.
    assert_eq!(NSTYLE, 1024);
    // The arena a `read(2)` lands in, and the read that fills it.
    assert_eq!(ARENA, 1 << 20);
    assert_eq!(READ_CHUNK, 4096);
}

/// **A terminal costs a fixed amount of memory, and this is it.**
///
/// Stated as one number so that a change to any structure has to come past it.
/// Everything is the caller's: `Storage` holds six borrowed slices and owns
/// nothing.
#[test]
fn the_whole_storage_requirement_is_fixed_and_stated() {
    let cell = core::mem::size_of::<PackedCell>();
    let arena = ARENA;
    let grid = GRID * cell;
    let cells3 = CELLS3 * cell;
    let tmpl = NSTYLE * core::mem::size_of::<Option<StyleWord>>();
    let tabs = MAX_COLS;
    let used = MAX_ROWS * core::mem::size_of::<RowExtent>();
    let total = arena + grid + cells3 + tmpl + tabs + used;
    // 1 MiB arena, 10 MiB grid, 2 MiB permutation table, 24 KB renditions.
    assert_eq!(arena, 1_048_576);
    assert_eq!(grid, 10_485_760);
    assert_eq!(cells3, 2_097_152);
    assert_eq!(tmpl, 24_576);
    assert_eq!(tabs, 2_048);
    assert!(
        total < 14 << 20,
        "the fixed requirement grew past 14 MiB: {total}"
    );
}
