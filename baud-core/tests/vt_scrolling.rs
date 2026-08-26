//! Vertical movement and scrolling: DECSTBM, IND, RI, NEL, LF, IL, DL, SU, SD.
//!
//! These operations are one feature, not eight. Every one of them either
//! respects the scrolling region or corrupts the screen, and three of them
//! (DL, DECSTBM, SM/RM) were previously *parsed and dropped* -- accepted by the
//! parser, discarded by `dispatch`, with no way for an application to notice.
//!
//! Assertions compare rendered rows rather than internal state, so a
//! representation change cannot make a test vacuously pass.

use baud_core::*;

const COLS: usize = 200;

fn drive(mode: OpMode, bytes: &[u8]) -> Box<Storage> {
    let mut s = Storage::boxed();
    {
        let mut t = Term::new(&mut s, mode);
        feed(&mut t, bytes);
    }
    s
}

fn feed(t: &mut Term, bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
}

/// The rendered rows of a run, as comparable values.
fn rows_of(mode: OpMode, bytes: &[u8]) -> Vec<Vec<PackedCell>> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, mode);
    feed(&mut t, bytes);
    let (_, rows) = t.geometry();
    (0..rows.index())
        .map(|y| t.row(Row::from_index(y)).to_vec())
        .collect()
}

fn cursor_of(mode: OpMode, bytes: &[u8]) -> (usize, usize) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, mode);
    feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

const MODES: [OpMode; 3] = [OpMode::Runs, OpMode::Computed, OpMode::Templated];

// ── column semantics ────────────────────────────────────────────────────────

/// IND (`ESC D`) is *index*: down one line, column untouched.
///
/// This routed through a `newline()` that reset the column, making IND behave
/// as NEL. `[[source: ctlseqs]]` separates them precisely.
#[test]
fn ind_preserves_the_column() {
    for m in MODES {
        assert_eq!(
            cursor_of(m, b"abcde\x1bD"),
            (1, 5),
            "{m:?}: IND must move down and leave the column alone"
        );
    }
}

/// NEL (`ESC E`) is CR + IND.
#[test]
fn nel_returns_to_column_one() {
    for m in MODES {
        assert_eq!(cursor_of(m, b"abcde\x1bE"), (1, 0), "{m:?}");
    }
}

/// LF moves down only, because LNM (mode 20) is reset by default.
#[test]
fn lf_does_not_return_to_column_one_by_default() {
    for m in MODES {
        assert_eq!(
            cursor_of(m, b"abcde\n"),
            (1, 5),
            "{m:?}: LF with LNM reset must not carriage-return"
        );
    }
}

/// With LNM set (`CSI 20 h`) it does. That is the whole difference.
#[test]
fn lf_returns_to_column_one_when_lnm_is_set() {
    for m in MODES {
        assert_eq!(cursor_of(m, b"\x1b[20habcde\n"), (1, 0), "{m:?}");
        // and resetting it restores the default
        assert_eq!(cursor_of(m, b"\x1b[20h\x1b[20labcde\n"), (1, 5), "{m:?}");
    }
}

// ── DECSTBM ─────────────────────────────────────────────────────────────────

/// `CSI r` sets the region and homes the cursor to its top-left.
#[test]
fn decstbm_homes_the_cursor_to_the_region_top() {
    for m in MODES {
        assert_eq!(cursor_of(m, b"\x1b[3;6r"), (2, 0), "{m:?}");
    }
}

/// A region of fewer than two lines is rejected, leaving the previous one.
#[test]
fn decstbm_rejects_a_degenerate_region() {
    for m in MODES {
        // 4;4 is one line: ignored, so the cursor is not homed to row 3.
        assert_eq!(cursor_of(m, b"\x1b[4;4r"), (0, 0), "{m:?}");
    }
}

/// Bare `CSI r` means the whole screen, which is not the same as `CSI 1;1 r`.
///
/// `OneBased::from_param` clamps an omitted parameter to 1, so the two were
/// indistinguishable until the bottom became an `Option`.
#[test]
fn bare_decstbm_resets_to_full_screen() {
    for m in MODES {
        // Shrink, then reset, then check a line feed at the old region bottom
        // does NOT scroll -- it would if the region were still 2..4.
        let with_reset = rows_of(m, b"\x1b[3;5r\x1b[r\x1b[5;1HX\n\nY");
        let never_set = rows_of(m, b"\x1b[5;1HX\n\nY");
        assert_eq!(
            with_reset, never_set,
            "{m:?}: bare CSI r must restore the full-screen region"
        );
    }
}

/// A line feed at the region bottom scrolls the region and nothing else.
#[test]
fn lf_at_region_bottom_scrolls_only_the_region() {
    for m in MODES {
        // Region rows 2..4 (1-based 3..5). Put a marker outside it, at row 0,
        // and fill the region so a scroll is observable.
        let bytes = b"\x1b[1;1HTOP\x1b[3;5r\x1b[3;1Ha\nb\nc\nd";
        let r = rows_of(m, bytes);
        let untouched = rows_of(m, b"\x1b[1;1HTOP");
        assert_eq!(
            r[0], untouched[0],
            "{m:?}: row 0 is outside the region and must not move"
        );
        for y in 5..r.len() {
            assert_eq!(r[y], untouched[y], "{m:?}: row {y} is below the region");
        }
    }
}

/// RI at the region top scrolls the region down, not the screen.
#[test]
fn ri_at_region_top_scrolls_only_the_region() {
    for m in MODES {
        let bytes = b"\x1b[1;1HTOP\x1b[3;5r\x1b[3;1H\x1bM";
        let r = rows_of(m, bytes);
        let untouched = rows_of(m, b"\x1b[1;1HTOP");
        assert_eq!(r[0], untouched[0], "{m:?}: RI must not scroll row 0");
        for y in 5..r.len() {
            assert_eq!(r[y], untouched[y], "{m:?}: row {y} is below the region");
        }
    }
}

// ── DL / IL / SU / SD ───────────────────────────────────────────────────────

/// DL was parsed and dropped. It must actually delete.
#[test]
fn dl_deletes_lines() {
    for m in MODES {
        let after = rows_of(m, b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[1;1H\x1b[M");
        let want = rows_of(m, b"\x1b[1;1Hbbb\r\nccc");
        assert_eq!(after, want, "{m:?}: CSI M must delete the cursor's line");
    }
}

/// DL with the cursor outside the region does nothing.
#[test]
fn dl_outside_the_region_is_a_no_op() {
    for m in MODES {
        let bytes = b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[2;4r\x1b[1;1H\x1b[M";
        let after = rows_of(m, bytes);
        let want = rows_of(m, b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[2;4r\x1b[1;1H");
        assert_eq!(after, want, "{m:?}");
    }
}

/// DL with a count larger than the region must clear it, not panic.
///
/// `delete_lines_in` computed `bot - n` on usize, which underflows when the
/// whole region is deleted from row 0.
#[test]
fn dl_with_a_huge_count_does_not_panic() {
    for m in MODES {
        let _ = drive(m, b"\x1b[1;1Hx\x1b[99999M");
        let _ = drive(m, b"\x1b[2;5r\x1b[2;1Hx\x1b[99999M");
        let _ = drive(m, b"\x1b[1;1Hx\x1b[99999L");
    }
}

/// SU and SD scroll the region without moving the cursor.
#[test]
fn su_and_sd_leave_the_cursor_alone() {
    for m in MODES {
        assert_eq!(cursor_of(m, b"\x1b[5;7H\x1b[2S"), (4, 6), "{m:?}: SU");
        assert_eq!(cursor_of(m, b"\x1b[5;7H\x1b[2T"), (4, 6), "{m:?}: SD");
    }
}

/// SU by one is the same screen as a line feed at the region bottom.
#[test]
fn su_agrees_with_a_line_feed_at_the_bottom() {
    for m in MODES {
        let via_su = rows_of(m, b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[S");
        let via_lf = rows_of(m, b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[50;1H\n");
        assert_eq!(via_su, via_lf, "{m:?}");
    }
}

/// IL must stop at the region bottom rather than the screen bottom.
#[test]
fn il_is_bounded_by_the_region() {
    for m in MODES {
        let bytes = b"\x1b[1;1Haaa\r\nbbb\r\nccc\r\nddd\x1b[1;3r\x1b[1;1H\x1b[L";
        let r = rows_of(m, bytes);
        let untouched = rows_of(m, b"\x1b[1;1Haaa\r\nbbb\r\nccc\r\nddd");
        assert_eq!(
            r[3], untouched[3],
            "{m:?}: row 3 is below a 1..3 region and must not be pushed"
        );
    }
}

// ── reset ───────────────────────────────────────────────────────────────────

/// RIS restores the full-screen region and the default modes.
///
/// A region surviving a reset is how an application ends up drawing into a
/// window it never asked for.
#[test]
fn ris_restores_the_region_and_modes() {
    for m in MODES {
        let after = rows_of(m, b"\x1b[3;5r\x1b[20h\x1bc\x1b[5;1HX\n\nY");
        let fresh = rows_of(m, b"\x1b[5;1HX\n\nY");
        assert_eq!(after, fresh, "{m:?}: RIS must restore region and LNM");
    }
}

/// Every row is `COLS` wide, so the row-slice assertions above compare like
/// with like. A stride change that broke this would make them vacuous.
#[test]
fn rows_are_full_width() {
    let r = rows_of(OpMode::Computed, b"x");
    assert!(!r.is_empty());
    for row in &r {
        assert_eq!(row.len(), COLS);
    }
}
