//! **A resize is a change of geometry, not of layout.**
//!
//! The grid's row pitch is fixed for the life of the storage -- `Term::stride`,
//! chosen by `store_stride` from the cache line -- so a change of width moves
//! no cell, a change of height moves cells only to undo a rotation, and what
//! a resize costs is the delta it blanks. These pin that contract from every
//! side: the pitch itself, the cell that survives, the cell that is blanked,
//! the cell that is not touched, and the cursor. The parked primary and the
//! shrink-after-scroll hazard are pinned beside the tests that found them, in
//! `text.rs` and `xterm/soft_reset_scope.rs`.

mod support;

use baud_core::*;
use baud_domain::*;

/// A 128-byte line, which is this class of machine; the pitch it yields is
/// pinned exhaustively in `baud-domain/tests/cache.rs`.
fn line128() -> CacheGeometry {
    CacheGeometry::new(Some(LineSize::of(128)), None, None, None)
}

fn text(vp: &[PackedCell], w: usize, y: usize, n: usize) -> String {
    vp[y * w..y * w + n]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

/// The pitch is the cache geometry's to choose, once, and never a resize's.
#[test]
fn the_pitch_is_the_cache_geometrys_and_survives_every_resize() {
    let mut owned = Owned::with_geometry(&support::CORPUS, &line128());
    let pitch = owned.stride();
    assert_eq!(
        pitch,
        grid_stride(&line128()),
        "one function decides the pitch"
    );
    assert_eq!(
        pitch.index(),
        2416,
        "MAX_COLS plus 23 lines of 128 B, the searched step"
    );
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    assert_eq!(
        t.stride(),
        pitch,
        "the terminal addresses by the storage's pitch"
    );
    for ws in [
        WinSize {
            ws_row: 10,
            ws_col: 33,
        },
        WinSize {
            ws_row: 50,
            ws_col: 200,
        },
        WinSize {
            ws_row: 3,
            ws_col: 2048,
        },
    ] {
        assert_eq!(t.resize(&ws), Resized::Ok);
        assert_eq!(
            t.stride(),
            pitch,
            "{}x{}: the pitch is not the geometry's to change",
            ws.ws_row,
            ws.ws_col
        );
        let (cols, rows) = t.geometry();
        assert!(cols.index() <= pitch.index(), "every width fits the pitch");
        assert_eq!(t.viewport().len(), rows.index() * pitch.index());
    }
    // With nothing detected the fallback is still an odd multiple of the
    // smallest line any cache uses, so it aliases no way on any machine.
    let unknown = Owned::new(&support::CORPUS).stride();
    assert_eq!(unknown, grid_stride(&CacheGeometry::UNKNOWN));
    assert_eq!(unknown.index(), 3896, "MAX_COLS plus 231 lines of 64 B");
}

/// A narrower screen keeps what fits, blanks what it lost, and lowers the
/// row's mark to the new width so blanking stays as cheap as it was.
#[test]
fn a_narrower_screen_keeps_what_fits_and_blanks_the_rest() {
    support::with_corpus(|t| {
        let line = "abcdefghij".repeat(20);
        support::feed(t, line.as_bytes());
        assert_eq!(t.row_extent(Row::ZERO).index(), 200);
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        assert_eq!(
            t.row_extent(Row::ZERO).index(),
            100,
            "the mark came down with the width"
        );
        let w = t.stride().index();
        let vp = t.viewport();
        assert_eq!(text(vp, w, 0, 100), line[..100]);
        assert!(
            vp[100..w].iter().all(|c| c.content.is_empty()),
            "the columns the screen lost are blank, to the pitch"
        );
    });
}

/// Growing shows blank where the screen grew, whatever stood there before it
/// shrank: the cells past the width are blanked on the way down, so the way
/// up finds nothing and touches nothing.
#[test]
fn a_wider_screen_shows_blank_where_it_grew() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[151GZ");
        assert!(
            t.row_extent(Row::ZERO).index() > 150,
            "the glyph landed past column 150"
        );
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert!(
            t.row_extent(Row::ZERO).index() <= 100,
            "the mark did not come back up"
        );
        let w = t.stride().index();
        let vp = t.viewport();
        assert!(
            vp[..w].iter().all(|c| c.content.is_empty()),
            "the glyph the screen lost is gone, and growing shows blank"
        );
    });
}

/// Both axes shrink, then both grow: the rows the screen lost were blanked
/// across the *old* width, so growing back finds nothing in the corner that
/// was outside both bounds at once. The perturbation that blanks lost rows
/// only to the new width passes every other test here.
#[test]
fn shrinking_both_axes_then_growing_shows_blank_in_the_corner() {
    support::with_corpus(|t| {
        // A glyph in the far corner: row 41, column 181.
        support::feed(t, b"\x1b[41;181HQ");
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 100,
            },
        );
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert!(
            t.viewport().iter().all(|c| c.content.is_empty()),
            "the corner the screen lost on both axes is blank"
        );
    });
}

/// **A change of width moves no cell and leaves the rotation alone.** The
/// raw grid, read in place under its rotation as a shader would, is
/// byte-identical on every physical row across a shrink and a growth.
#[test]
fn a_change_of_width_moves_no_cell_and_keeps_the_rotation() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        let rot = t.rotation().index();
        assert_ne!(rot, 0, "the screen has scrolled");
        let s = t.stride().index();
        let before = t.cells().to_vec();
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 150,
            },
        );
        assert_eq!(
            t.rotation().index(),
            rot,
            "the width is not the rotation's to change"
        );
        assert_eq!(t.stride().index(), s);
        let after = t.cells();
        for p in 0..50 {
            assert_eq!(
                &after[p * s..p * s + 150],
                &before[p * s..p * s + 150],
                "physical row {p} is where it was"
            );
        }
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert_eq!(t.rotation().index(), rot);
        assert_eq!(
            t.cells(),
            &before[..],
            "nothing past column 150 held a cell to lose, so the grid is what it was"
        );
    });
}

/// The cursor is clamped into the grid that exists, not sent home: there is
/// content to be relative to now.
#[test]
fn the_cursor_is_clamped_not_homed() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[40;150H");
        assert_eq!(t.cursor(), (Row::from_index(39), Col::at_index(149)));
        support::shrink(
            t,
            &WinSize {
                ws_row: 20,
                ws_col: 100,
            },
        );
        assert_eq!(
            t.cursor(),
            (Row::from_index(19), Col::at_index(99)),
            "clamped to the last cell of the new grid"
        );
        support::feed(t, b"\x1b[5;5H");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(4)),
            "a cursor inside the new grid stays put"
        );
    });
}
