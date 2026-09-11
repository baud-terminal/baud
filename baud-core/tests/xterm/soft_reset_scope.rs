//! What `DECSTR` resets, and what it must leave alone.
//!
//! `CSI ! p` is a DEC function, but two of the things baud used to reset with
//! it — the alternate screen and mouse tracking — are **xterm extensions**. A
//! DEC manual's list of what DECSTR resets cannot mention them, so xterm is
//! the authority, and in `ReallyReset` both sit inside the `if (full)` branch:
//! `FromAlternate` next to `ClearScreen`, and `send_mouse_pos = MOUSE_OFF`
//! next to `extend_coords = 0`. Nothing mouse-related happens on the soft path
//! at all.
//!
//! baud reset all four, so a full-screen application sending `CSI ! p` to
//! recover a confused terminal was dropped onto the primary screen mid-draw,
//! and the primary's saved contents went with it (`alt_saved = None`).
//!
//! Found by core2's alternate-screen isolation property once the generator
//! could emit `CSI ! p` at all — it shrank to `[Decstr, Sgr(1), El(0), ...]`.
//! DECSTR is a `done` row and had been for a long time; what was missing was
//! an input shape, not an implementation.

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, bool) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    support::feed(&mut t, bytes);
    let alt = t.screen() == Screen::Alternate;
    (t.viewport().to_vec(), alt)
}

/// **DECSTR does not leave the alternate screen.**
#[test]
fn soft_reset_stays_on_the_alternate_screen() {
    let (_, alt) = run(b"\x1b[?1049h\x1b[!p");
    assert!(alt, "CSI ! p must not switch back to the primary screen");
}

/// And the primary's contents survive it.
///
/// This is the part that made the old behaviour destructive rather than merely
/// wrong: `alt_saved = None` discarded what `?1049 h` had saved, so leaving
/// the alt screen afterwards could not restore the primary.
#[test]
fn the_primary_screen_survives_a_soft_reset_on_the_alternate() {
    let baseline = run(b"PRIMARY").0;
    let after = run(b"PRIMARY\x1b[?1049h\x1b[!pALT\x1b[?1049l").0;
    assert_eq!(
        after, baseline,
        "the primary must come back exactly as it was"
    );
}

/// **DECSTR does not reset mouse tracking or its encoding.**
///
/// `send_mouse_pos` and `extend_coords` are both RIS-only in xterm. An
/// application that enables SGR mouse reporting and later soft-resets would
/// otherwise stop receiving the events it is still waiting for.
#[test]
fn soft_reset_leaves_mouse_tracking_alone() {
    let (tracking, reply) = mouse_state(b"\x1b[?1003h\x1b[?1006h\x1b[!p");
    assert_ne!(
        tracking,
        MouseTracking::Off,
        "CSI ! p must not turn mouse reporting off"
    );
    // DECRQM is the observable channel for the encoding: `1` means set.
    assert_eq!(
        reply,
        b"\x1b[?1006;1$y".to_vec(),
        "nor revert the encoding to X10"
    );
}

/// Drive `bytes`, then ask DECRQM about `?1006`.
fn mouse_state(bytes: &[u8]) -> (MouseTracking, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    for chunk in [bytes, &b"\x1b[?1006$p"[..]] {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index((chunk).len()));
    }
    (t.mouse_tracking(), t.reply().to_vec())
}

/// RIS still resets all of it — that is the difference between the two.
#[test]
fn a_hard_reset_still_resets_everything() {
    let (_, alt) = run(b"\x1b[?1049h\x1bc");
    assert!(!alt, "RIS returns to the primary screen");

    let (tracking, reply) = mouse_state(b"\x1b[?1003h\x1b[?1006h\x1bc");
    assert_eq!(tracking, MouseTracking::Off, "RIS clears tracking");
    assert_eq!(reply, b"\x1b[?1006;2$y".to_vec(), "and the encoding");
}

/// **Shrinking the window after a scroll must not panic.**
///
/// `RowRot` is bounded by the row count at the time it was advanced, so a
/// shrink can leave it at or past the new one. `View::phys` is a single
/// conditional subtract -- deliberately, because both operands are normally
/// below `rows` -- so past that point it names a row outside the grid:
/// `clear_screen` blanks rows nobody can see, and `viewport`'s `rotate_left`
/// panics outright with `mid <= self.len()`.
///
/// The reproduction is a full-screen application and a user dragging a corner.
/// It was unreachable from the property suite because `resize` was never
/// called by any generated program -- an entire public entry point with no
/// coverage, which is why `Op::Resize` now exists.
#[test]
fn a_shrink_after_scrolling_does_not_panic() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    // 50 rows: 49 newlines reach the last row, the remainder scroll, so this
    // leaves the rotation at 41 -- comfortably past the ten rows below.
    let payload: Vec<u8> = (0..90).flat_map(|_| b"X\r\n".to_vec()).collect();
    t.write_at(BytePos::ZERO, &payload);
    t.feed(BytePos::ZERO, ReadLen::from_index((&payload).len()));

    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 200,
        }),
        Resized::Ok
    );

    // Both of these panicked before the fix.
    let w = t.stride().index();
    let vp = t.viewport();
    assert_eq!(vp.len(), 10 * w, "ten rows of the live stride");
    // Here the stride *is* the width, so every cell in view is grid -- there
    // is no zeroed tail to except, which is one thing the live stride
    // simplifies. What matters is still that no content survived.
    assert!(
        !vp.iter()
            .any(|c| c.content.as_codepoint().map(|v| v.value()) == Some(u32::from(b'X'))),
        "resize clears: no scrolled-off text may remain visible"
    );
}

/// Growing is the same hazard from the other side, and must also be clean.
#[test]
fn a_grow_after_scrolling_does_not_panic() {
    // **Provisioned for 120 rows, because growing needs somewhere to grow.**
    // `Term::resize` answers `Truncated` past the row capacity rather than
    // allocating behind the caller -- the storage is the caller's here, where
    // `baud-core2`'s is a fixed array it can always grow into.
    support::with_room(120, |t| {
        let payload: Vec<u8> = (0..90).flat_map(|_| b"Y\r\n".to_vec()).collect();
        support::feed(t, &payload);

        assert_eq!(
            t.resize(&WinSize {
                ws_row: 120,
                ws_col: 200,
            }),
            Resized::Ok
        );
        let w = t.stride().index();
        let vp = t.viewport();
        assert_eq!(vp.len(), 120 * w);
        assert!(
            !vp.iter()
                .any(|c| c.content.as_codepoint().map(|v| v.value()) == Some(u32::from(b'Y'))),
            "and the same growing"
        );
    });
}

/// **A cursor saved before a resize is restored into the grid that exists.**
///
/// `resize` homes the cursor but cannot touch a position DECSC already stored,
/// so `ESC 7` on a 50-row screen and `ESC 8` on a one-row screen used to put
/// `cy` at 49 — outside the grid. That is not only a wrong CPR answer, which
/// `Term::cursor`'s own doc rules out for the column ("a 200-column page has
/// no column 201"); writes and erases index with `cy`, and `View::phys` is a
/// single conditional subtract that assumes it is in range.
///
/// Found by core2's resize-between-reads property, which shrank to
/// `[CupHuge, Decslrm]` / `[Restore]` at one row by one column.
#[test]
fn a_cursor_saved_before_a_resize_is_clamped_on_restore() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    feed(&mut t, b"\x1b[99999;99999H\x1b7");
    assert_eq!(t.cursor().0.index(), 49, "saved from the last row of fifty");

    assert_eq!(
        t.resize(&WinSize {
            ws_row: 1,
            ws_col: 1,
        }),
        Resized::Ok
    );
    feed(&mut t, b"\x1b8");
    let (r, c) = t.cursor();
    assert_eq!(r.index(), 0, "restored row is inside the one-row grid");
    assert!(c.index() <= 1, "and so is the column");
}

/// The clamp must not flatten an ordinary save/restore.
///
/// `cx` clamps to `cols`, not `cols - 1`, because with autowrap on the cursor
/// legitimately sits one past the last column until the next write consumes
/// it. A restore on an unchanged grid has to give that state back.
#[test]
fn save_and_restore_round_trips_when_the_grid_has_not_changed() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    feed(&mut t, b"\x1b[10;20H\x1b7\x1b[1;1H");
    assert_eq!((t.cursor().0.index(), t.cursor().1.index()), (0, 0));
    feed(&mut t, b"\x1b8");
    assert_eq!(
        (t.cursor().0.index(), t.cursor().1.index()),
        (9, 19),
        "an unresized restore is exact"
    );
}

/// **Resizing while on the alternate screen clears the primary too.**
///
/// `resize` discards the row rotation, which is only sound if the buffer that
/// rotation described is cleared with it. `clear_screen` clears whichever
/// screen is current, so a resize taken on the alternate screen left the
/// primary holding rotated content with `rot` back at zero: every row
/// displaced. The path is ordinary — a full-screen application is *on* the
/// alternate screen, which is when a user resizes the window.
#[test]
fn a_resize_on_the_alternate_screen_clears_the_primary_too() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    // Scroll the primary so the rotation is non-zero, then mark it.
    let payload: Vec<u8> = (0..60).flat_map(|_| b"x\r\n".to_vec()).collect();
    feed(&mut t, &payload);
    feed(&mut t, b"MARK");

    feed(&mut t, b"\x1b[?1049h");
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 20,
            ws_col: 100,
        }),
        Resized::Ok
    );
    feed(&mut t, b"\x1b[?1049l");

    assert!(
        !t.viewport().iter().any(|c| c
            .content
            .as_codepoint()
            .is_some_and(|v| v.value() == u32::from(b'x'))),
        "the primary must be cleared, not left rotated with the rotation \
         forgotten"
    );
}

/// `?1049 l` restores the cursor through the same clamp DECRC uses.
///
/// `[[source: ctlseqs]]` defines it as restoring "cursor as in DECRC", so it
/// *is* DECRC and must not be a second implementation. It was one, and the
/// second lacked the clamp — so a save taken before a resize put `cy` outside
/// the grid, which writes and erases index with.
#[test]
fn the_alt_screen_restore_clamps_like_decrc() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    // Park the cursor on row 49, enter the alt screen (which saves it), then
    // shrink to a single row and come back.
    feed(&mut t, b"\x1b[50;100H\x1b[?1049h");
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 1,
            ws_col: 1,
        }),
        Resized::Ok
    );
    feed(&mut t, b"\x1b[?1049l");

    let (r, c) = t.cursor();
    assert_eq!(r.index(), 0, "restored row is inside the one-row grid");
    assert!(c.index() <= 1, "and so is the column");
}

/// **`Term::row` past the last row returns cells, not an error.**
///
/// The buffer is `MAX_ROWS` deep and the grid is `rows` deep, so a row index
/// between them is allocated memory that is not part of the grid: `row(55)` on
/// a fifty-row screen returns real cells from physical row 55. No panic, no
/// `None` — silently the wrong data, which is the shape this project rules out
/// for replies.
///
/// Pinned rather than changed. `row` returns `&[PackedCell]`, so making it
/// answer an out-of-range index honestly means either returning `Option` or
/// panicking, and both change a published signature or its behaviour. That is
/// the owner's call, not a fix to slip into a testing pass. `viewport()` is
/// the bounded accessor and is what the benchmarks and tests use.
#[test]
fn row_past_the_grid_is_a_documented_sharp_edge() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let t = Term::new(s.storage(), &support::CORPUS);
    let (cols, rows) = t.geometry();
    assert_eq!(rows.index(), 50);

    // In range: the row is `cols` wide.
    assert_eq!(t.row(Row::from_index(rows.index() - 1)).len(), cols.index());

    // Past the grid but inside the allocation: still a slice, still `cols`
    // wide, and not part of the screen. If this ever starts returning `None`
    // or panicking, that is a deliberate API change and this test should be
    // updated to say so rather than deleted.
    assert_eq!(t.row(Row::from_index(rows.index())).len(), cols.index());
    assert_eq!(t.row(Row::from_index(rows.index() + 5)).len(), cols.index());
}

/// `get_row` is the checked half of the pair, and answers honestly.
///
/// Additive: it does not change what `row` does, so nothing published moves.
/// It exists so a caller that cannot prove its index is in range has something
/// to call that is not silently wrong -- an embedder holding a row count
/// across a resize, which is the exact staleness that turned a shrink into a
/// panic in `viewport` and an out-of-grid cursor in DECRC.
#[test]
fn get_row_bounds_what_row_takes_on_trust() {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let (cols, rows) = t.geometry();

    assert_eq!(
        t.get_row(Row::from_index(rows.index() - 1)).map(<[_]>::len),
        Some(cols.index()),
        "the last row is on the grid"
    );
    assert_eq!(
        t.get_row(Row::from_index(rows.index())),
        None,
        "one past it is not, and says so"
    );
    assert_eq!(t.get_row(Row::from_index(rows.index() + 5)), None);

    // And it tracks a resize, which is the case it exists for.
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 80,
        }),
        Resized::Ok
    );
    assert!(
        t.get_row(Row::from_index(9)).is_some(),
        "inside the new grid"
    );
    assert_eq!(
        t.get_row(Row::from_index(10)),
        None,
        "a row that existed before the shrink does not now"
    );
}

/// `stride` is the row pitch, and it is **not** the column count.
///
/// Every row is `MAX_COLS` wide in memory however narrow the grid is, so
/// `viewport().len()` is `rows * stride`, not `rows * cols`. Confusing the two
/// is the `RowBlock` mistake this project already typed its way out of --
/// "operands the wrong way round, a plausible number and a silently wrong
/// slice" -- and it stays true across a resize, which is what makes it usable
/// as a divisor by an embedder walking rows.
#[test]
fn stride_is_the_row_pitch_and_here_it_is_the_column_count() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let (cols, rows) = t.geometry();

    // **The inverse of `baud-core2`'s claim, and deliberately so.** core2
    // strides by `MAX_COLS`, so a 200-column grid is 2048 wide in memory. This
    // core strides by the live column count, which is worth +14% on every
    // payload that scrolls: at 2048 cells a row, consecutive rows landed in
    // the same L1 sets and any operation touching several fought an eight-way
    // set with fifty of them.
    //
    // The *contract* is unchanged and is why `stride()` is public at all: it
    // is the row pitch, and an embedder divides by it. What changed is the
    // number, so a test that hard-codes 2048 reads the wrong rows.
    assert_eq!(t.stride(), cols, "the pitch is the live width here");
    assert_eq!(
        t.viewport().len(),
        rows.index() * t.stride().index(),
        "the viewport is rows by stride"
    );

    let before = t.stride();
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 7,
            ws_col: 33,
        }),
        Resized::Ok
    );
    // The inverse again: this core's pitch *is* the geometry, so a resize
    // moves it. `before` is what it was, and it must have changed with `cols`.
    assert_ne!(t.stride(), before, "the pitch follows the geometry here");
    assert_eq!(t.viewport().len(), 7 * t.stride().index());
}

/// **The same shrink, on the alternate screen.**
///
/// `a_shrink_after_scrolling_does_not_panic` covers `rot`. The alternate screen
/// now carries a second rotation of its own, `alt_rot`, and it can go stale the
/// identical way: it is bounded by the *old* row count, `View::phys` is a
/// single conditional subtract that assumes both operands are below `rows`, and
/// `viewport`'s `rotate_left` panics outright on a rotation larger than the
/// slice.
///
/// `resize` discards both through `clear_rotations`, and this is the test that
/// says so. Without the alternate arm it would be exactly the gap the primary's
/// version was written to close, one screen over -- and `CLAUDE.md` records
/// `Term::resize` as an entry point that had **no caller anywhere** until a
/// test reached it, at which point it yielded three bugs including this panic.
#[test]
fn a_shrink_after_scrolling_the_alternate_screen_does_not_panic() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    // `?1049 h` first, so every newline below rotates `alt_rot` rather than
    // `rot`. 90 lines against 50 rows leaves it at 41, past the ten below.
    let mut payload: Vec<u8> = b"\x1b[?1049h".to_vec();
    payload.extend((0..90).flat_map(|_| b"X\r\n".to_vec()));
    t.write_at(BytePos::ZERO, &payload);
    t.feed(BytePos::ZERO, ReadLen::from_index((&payload).len()));

    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 200,
        }),
        Resized::Ok
    );

    let w = t.stride().index();
    let vp = t.viewport();
    assert_eq!(vp.len(), 10 * w, "ten rows of the live stride");
    assert!(
        !vp.iter()
            .any(|c| c.content.as_codepoint().map(|v| v.value()) == Some(u32::from(b'X'))),
        "resize clears both buffers: no scrolled-off text may remain visible"
    );
}
