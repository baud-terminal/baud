//! **A resize is a change of geometry, not of layout.**
//!
//! The grid's row pitch is fixed for the life of the storage -- `Term::stride`,
//! chosen by `store_stride` from the cache line -- so a change of width moves
//! no cell, a change of height moves cells only to undo a rotation, and a
//! smaller window hides what lies past its edge rather than losing it. These
//! pin that contract from every side: the pitch itself, the cell that comes
//! back, the cell that must not (a line cleared or reused while the screen
//! was narrow), the cell that is not touched, the cursor, and the parked
//! primary under an alternate screen that grows. The shrink-after-scroll
//! hazard is pinned beside the test that found it, in `text.rs` and
//! `xterm/soft_reset_scope.rs`.

mod support;

use baud_core::*;
use baud_domain::*;

/// A 128-byte line, which is this class of machine; the pitch it yields is
/// pinned exhaustively in `baud-domain/tests/cache.rs`.
fn line128() -> CacheGeometry {
    CacheGeometry::new(Some(LineSize::of(128)), None, None, None)
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

fn rows_text(t: &Term<'_>, rows: usize, n: usize) -> Vec<String> {
    (0..rows)
        .map(|y| {
            t.row(Row::from_index(y))[..n]
                .iter()
                .map(|c| match c.content.as_codepoint() {
                    Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                    None => ' ',
                })
                .collect()
        })
        .collect()
}

/// **A smaller window hides; a larger one shows it again, exactly.** Sixty
/// numbered lines on a fifty-row screen, so the grid is rotated; shrink to
/// ten by a hundred, grow back, and every row reads as it did -- read
/// through `row`, which applies the rotation without undoing it, so the
/// resize is what un-rotates.
#[test]
fn a_smaller_window_hides_and_a_larger_one_shows_it_again() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i} {}\r\n", "x".repeat(150)).into_bytes())
            .collect();
        support::feed(t, &lines);
        assert_ne!(t.rotation().index(), 0, "the screen has scrolled");
        let before = rows_text(t, 50, 200);
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 100,
            },
        );
        assert_eq!(
            rows_text(t, 10, 100),
            before[..10]
                .iter()
                .map(|s| s[..100].to_string())
                .collect::<Vec<_>>(),
            "the top-left of the screen is what is live"
        );
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert_eq!(
            rows_text(t, 50, 200),
            before,
            "what was hidden is back where it was"
        );
    });
}

/// A glyph in the far corner survives a shrink on both axes and a growth
/// back: the corner was hidden, not lost.
#[test]
fn shrinking_both_axes_then_growing_restores_the_corner() {
    support::with_corpus(|t| {
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
        let text = rows_text(t, 41, 200);
        assert_eq!(&text[40][180..181], "Q", "the corner came back");
        assert!(text[..40].iter().all(|r| r.trim().is_empty()));
    });
}

/// **A line cleared while the screen was narrow does not bring its tail
/// back**, and the blank it was cleared to does not either: `EL` runs
/// through the mark, and a styled blank past the old edge is not mistaken
/// for the blank that follows it.
#[test]
fn a_line_cleared_while_narrow_comes_back_clean() {
    support::with_corpus(|t| {
        support::feed(t, "A".repeat(200).as_bytes());
        support::feed(t, b"\r\nB");
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        // Row 0 erased to a red blank; row 1 erased to the default.
        support::feed(t, b"\x1b[1;1H\x1b[41m\x1b[2K\x1b[0m\x1b[2;1H\x1b[2K");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        let text = rows_text(t, 2, 200);
        assert!(
            text[1].trim().is_empty(),
            "row 1: the erase reached the tail"
        );
        assert!(
            text[0].trim().is_empty(),
            "row 0: the erase reached the tail"
        );
        // Now the default erase over row 0 at full width must reach every
        // cell -- the red band left past the old edge is the case a fill that
        // trusted the mark would skip.
        support::feed(t, b"\x1b[1;1H\x1b[2K");
        let row0 = t.row(Row::ZERO);
        assert!(
            row0[..200]
                .iter()
                .all(|c| t.style_of(*c) == StyleWord::DEFAULT),
            "row 0: no red blank survives a default erase"
        );
    });
}

/// A row reused by a scroll while the screen was narrow comes back with the
/// new line and no old tail: the scroll's blank runs through the mark.
#[test]
fn a_row_reused_by_a_scroll_while_narrow_comes_back_clean() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..50)
            .flat_map(|i| format!("{i:02}{}\r\n", "A".repeat(198)).into_bytes())
            .collect();
        // Forty-nine newlines fill the screen without scrolling; the fiftieth
        // scrolls once, so the top row is reused and the rest keep their tails.
        support::feed(t, &lines);
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 100,
            },
        );
        support::feed(t, b"new\r\n");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        let text = rows_text(t, 10, 200);
        // `new` was typed over a line of `A`s without clearing it, so that
        // row keeps its tail -- overwrite semantics, as at any width. The row
        // the scroll brought in is the one that must be clean to the pitch.
        assert!(
            text[8].starts_with("newA"),
            "the new line is on screen: {:?}",
            &text[8][..10]
        );
        assert!(
            text[9].trim().is_empty(),
            "the row the scroll reused carries no tail"
        );
        assert!(text[7].ends_with('A'), "the row above kept its tail");
    });
}

/// **The alternate screen growing over hidden primary rows does not eat
/// them.** Shrink on the primary, enter the alternate screen, grow taller
/// there and draw on the exposed rows, leave: the primary is whole.
#[test]
fn the_parked_primary_survives_an_alternate_screen_that_grows() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..49)
            .flat_map(|i| format!("primary {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        let before = rows_text(t, 50, 200);
        support::shrink(
            t,
            &WinSize {
                ws_row: 20,
                ws_col: 200,
            },
        );
        support::feed(t, b"\x1b[?1049h");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        support::feed(t, b"\x1b[35;1HALT SCREEN WROTE HERE\x1b[45;1HAND HERE");
        assert!(rows_text(t, 50, 200)[34].starts_with("ALT SCREEN"));
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(rows_text(t, 50, 200), before, "every primary row is back");
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

/// **A blank left past the old edge is not mistaken for the blank beyond
/// it.** A red erase while narrow runs through the mark of a line that
/// reached column 200, so red now stands where the line stood, with the
/// default blank past it. Grown to 250, a default erase finds the default
/// blank at the live edge and would skip everything from the mark on --
/// which is why the mark was set to where the red stops, not to zero.
#[test]
fn a_blank_left_past_the_old_edge_is_not_mistaken_for_the_blank_beyond_it() {
    support::with_corpus(|t| {
        support::feed(t, "A".repeat(200).as_bytes());
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        support::feed(t, b"\x1b[1;1H\x1b[41m\x1b[2K\x1b[0m");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 250,
            }),
            Resized::Ok
        );
        support::feed(t, b"\x1b[1;1H\x1b[2K");
        let row = t.row(Row::ZERO);
        assert!(
            row[..250]
                .iter()
                .all(|c| t.style_of(*c) == StyleWord::DEFAULT),
            "no red survives a default erase over the whole row"
        );
    });
}

/// **A wider screen raises the marks to the old edge.** A red erase while
/// narrow, on a row with nothing past the edge, leaves the mark at zero and
/// red to column 100; grown to 200, the row's last cell is the default
/// blank, and a default erase that trusted the mark would stop at zero and
/// leave the band.
#[test]
fn a_wider_screen_raises_the_marks_so_a_band_to_the_old_edge_is_erased() {
    support::with_corpus(|t| {
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        support::feed(t, b"\x1b[3;1H\x1b[41m\x1b[2K\x1b[0m");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        support::feed(t, b"\x1b[3;1H\x1b[2K");
        let row = t.row(Row::from_index(2));
        assert!(
            row[..200]
                .iter()
                .all(|c| t.style_of(*c) == StyleWord::DEFAULT),
            "the red band to the old edge is erased"
        );
    });
}

/// **The park copies a row to its mark, not to the live width.** A line to
/// column 200, hidden past a shrink to 100; the alternate screen grows to
/// 200 and draws over the columns the tail lived in; the exit brings the
/// tail back, because the park had it.
#[test]
fn the_parked_primary_survives_an_alternate_screen_that_grows_wider() {
    support::with_corpus(|t| {
        let line = "abcdefghij".repeat(20);
        support::feed(t, line.as_bytes());
        support::shrink(
            t,
            &WinSize {
                ws_row: 50,
                ws_col: 100,
            },
        );
        support::feed(t, b"\x1b[?1049h");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        support::feed(t, format!("\x1b[1;1H{}", "X".repeat(200)).as_bytes());
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(rows_text(t, 1, 200)[0], line, "the whole line is back");
    });
}
