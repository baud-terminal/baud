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
//! and the primary's saved contents went with it (the `?1049` save emptied).
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
    (support::screen(&t), alt)
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
/// wrong: emptying the `?1049` save discarded what `?1049 h` had saved, so leaving
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
    t.feed(BytePos::ZERO, ReadLen::from_index(payload.len()));

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
    assert_eq!(vp.len(), 10 * w, "ten rows of the pitch");
    // The cursor's ten rows survive -- nine of `X` and the blank row it
    // stood on -- and the rows above them retired to the history.
    for y in 0..9 {
        let row = &vp[y * w..y * w + 200];
        assert_eq!(
            row[0].content.as_codepoint().map(|v| v.value()),
            Some(u32::from(b'X')),
            "row {y}"
        );
        assert!(
            row[1..].iter().all(|c| c.content.is_empty()),
            "row {y} holds one X"
        );
    }
    assert!(
        vp[9 * w..10 * w].iter().all(|c| c.content.is_empty()),
        "the cursor's row"
    );
    assert_eq!(
        t.scrollback().len(),
        40 + 41,
        "forty retired here, forty-one by the scrolls"
    );
}

/// Growing is the same hazard from the other side, and must also be clean.
#[test]
fn a_grow_after_scrolling_does_not_panic() {
    // **Provisioned for 120 rows, because growing needs somewhere to grow.**
    // `Term::resize` answers `Truncated` past the row capacity rather than
    // allocating behind the caller -- the storage is the caller's here, where
    // `baud-core2`'s was a fixed array it could always grow into.
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
        // Forty-one lines scrolled off and come back above the fifty that
        // were on screen: 90 of `Y`, the blank line the cursor was on, and
        // twenty-nine blank rows the history could not fill.
        for y in 0..90 {
            assert_eq!(
                vp[y * w].content.as_codepoint().map(|v| v.value()),
                Some(u32::from(b'Y')),
                "row {y}"
            );
        }
        assert!(
            vp[90 * w..].iter().all(|c| c.content.is_empty()),
            "and the same growing: blank below what the history restored"
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
/// rotation described has to go with it. `resize` un-rotates the parked
/// primary over its old height and clips it like the live surface, so it
/// comes back in order; a version that forgot the rotation left every row
/// displaced, and one that cleared the live screen left the primary alone. The path is ordinary — a full-screen application is *on* the
/// alternate screen, which is when a user resizes the window.
#[test]
fn a_resize_on_the_alternate_screen_reshapes_the_primary_too() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    // Scroll the primary so the rotation is non-zero, numbering the lines so
    // a displaced row is visible, then mark the last row.
    let payload: Vec<u8> = (0..60)
        .flat_map(|i| format!("{i:02}\r\n").into_bytes())
        .collect();
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

    let w = t.stride().index();
    let vp = t.viewport();
    // Eleven scrolls, so row `y` was line `11 + y`. The cursor was on the
    // last row, so the twenty rows that survive are the last twenty -- lines
    // 41 to 59 and the mark -- with the thirty above them retired to the
    // history, in order.
    for y in 0..19 {
        let text: String = vp[y * w..y * w + 2]
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .map(|v| char::from_u32(v.value()).unwrap_or('?'))
            .collect();
        assert_eq!(
            text,
            format!("{:02}", 41 + y),
            "row {y}: the primary came back in order, around the cursor"
        );
    }
    let mark: String = vp[19 * w..19 * w + 4]
        .iter()
        .filter_map(|c| c.content.as_codepoint())
        .map(|v| char::from_u32(v.value()).unwrap_or('?'))
        .collect();
    assert_eq!(mark, "MARK", "the cursor's row is the last one");
    assert_eq!(
        t.scrollback().len(),
        11 + 30,
        "eleven by scrolls, thirty by the height"
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

/// **`Term::row` past the last row is `None`**, an `Option` rather than
/// a panic.
///
/// The grid is a ring of rows, the screen a window into it, and
/// `Viewport::row` does not bound its index by the screen: a row index past
/// the last one, short of the ring's length, names a ring slot after the
/// screen's bottom, a retained history line or one nothing has written.
/// `Term::row` read through it and returned those cells, silently, which is
/// the shape this project rules out for replies. It absorbed `get_row`, the
/// checked form, and the bound tracks a resize, the stale row count an
/// embedder holds across one.
#[test]
fn row_past_the_grid_is_none() {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let (cols, rows) = t.geometry();
    assert_eq!(rows.index(), 50);

    assert_eq!(
        t.row(Row::from_index(rows.index() - 1)).map(<[_]>::len),
        Some(cols.index()),
        "the last row is on the grid"
    );
    // The control: the slot one past the screen is there to be read, and
    // `Viewport::row`, which is not bounded, still reads it -- so the `None`
    // below is the bound, not a missing slot.
    assert_eq!(
        t.viewport().row(Row::from_index(rows.index())).len(),
        cols.index(),
        "the ring slot past the screen"
    );
    assert_eq!(
        t.row(Row::from_index(rows.index())),
        None,
        "one past it is not, and says so"
    );
    assert_eq!(t.row(Row::from_index(rows.index() + 5)), None);

    // And it tracks a resize.
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 80,
        }),
        Resized::Ok
    );
    assert_eq!(
        t.row(Row::from_index(9)).map(<[_]>::len),
        Some(80),
        "inside the new grid"
    );
    assert_eq!(
        t.row(Row::from_index(10)),
        None,
        "a row that existed before the shrink does not now"
    );
}

/// `stride` is the row pitch, and it is **not** the column count.
///
/// Every row is the pitch wide in memory however narrow the grid is, so
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

    // **The pitch is neither `baud-core2`'s nor the live width.** core2 strode
    // by `MAX_COLS`, and 2048 cells is exactly one 16 KB L1 way, so every row
    // of a 200-column screen landed in the same sets and any operation
    // touching several fought an eight-way set with fifty of them. This core
    // then strode by the live width -- +14% on everything that scrolls -- and
    // a resize re-packed the grid. Now the pitch is fixed for the life of the
    // storage: at least the width provisioned -- not `MAX_COLS`, since with
    // the grid as the scrollback ring every line costs a row of the pitch --
    // an odd number of lines so it aliases no way, at a step that spreads the
    // rows over the sets -- so it keeps the 14% and a resize moves no cell.
    //
    // The *contract* is why `stride()` is public at all: it is the row pitch,
    // and an embedder divides by it. A test that hard-codes 2048, or the
    // width, reads the wrong rows.
    // Not `>`: the pitch is the provisioned width's own lines when they are
    // an odd count, which 200 columns on a 64-byte line is.
    assert!(
        t.stride().index() >= cols.index(),
        "the pitch admits the width provisioned"
    );
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
    // A resize changes the geometry and not the pitch: the rows stay where
    // they are and only the live width of each moves.
    assert_eq!(t.stride(), before, "the pitch survives the geometry");
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
/// `resize` un-rotates both over the old height, and this is the test that says
/// so. Without the alternate arm it would be exactly the gap the primary's
/// version was written to close, one screen over -- and `CONTRIBUTING.md`
/// records `Term::resize` as an entry point that had **no caller anywhere**
/// until a test reached it, at which point it yielded three bugs including this
/// panic.
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
    t.feed(BytePos::ZERO, ReadLen::from_index(payload.len()));

    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 200,
        }),
        Resized::Ok
    );

    let w = t.stride().index();
    let vp = t.viewport();
    assert_eq!(vp.len(), 10 * w, "ten rows of the pitch");
    for y in 0..10 {
        assert_eq!(
            vp[y * w].content.as_codepoint().map(|v| v.value()),
            Some(u32::from(b'X')),
            "row {y}: the top of the alternate screen survives the height"
        );
    }
}

/// **A full reset issued on the alternate screen is the power-on state, not
/// the state the shell had before the program started.** `RIS` soft-reset
/// first and then left the alternate screen, and leaving restores what
/// `?1049 h` saved -- the rendition, the charsets, DECAWM, DECOM and DECSCA's
/// protection -- over the reset it had just done, and cleared the primary in
/// the restored rendition. xterm's hard branch leaves the alternate screen
/// and then puts the flags back to their initial values
/// (`[[source: xterm-reset]]`), so nothing the entry saved survives it. Each
/// arm runs from the primary as the control, where there is nothing to
/// restore.
#[test]
fn a_hard_reset_on_the_alternate_screen_leaves_the_power_on_state() {
    for (enter, arm) in [
        (&b""[..], "from the primary"),
        (&b"\x1b[?1049h"[..], "from the alternate screen"),
    ] {
        let ws = support::CORPUS;
        let with = |setup: &[u8], after: &[u8], f: &dyn Fn(&Term<'_>, &[PackedCell])| {
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            support::feed(&mut t, setup);
            support::feed(&mut t, enter);
            support::feed(&mut t, b"\x1bc");
            support::feed(&mut t, after);
            let screen = support::screen(&t);
            f(&t, &screen);
        };
        // The rendition: blue set before the program, then the reset.
        with(b"\x1b[44m", b"x", &|t, s| {
            assert_eq!(
                s[0].content.as_codepoint().map(|c| c.value()),
                Some(u32::from('x'))
            );
            assert_eq!(
                t.style_of(s[0]),
                StyleWord::DEFAULT,
                "{arm}: the next character is in the default rendition"
            );
            assert_eq!(
                t.style_of(s[1]),
                StyleWord::DEFAULT,
                "{arm}: the reset cleared the screen in the default rendition"
            );
        });
        // The charset: DEC line drawing in G0.
        with(b"\x1b(0", b"q", &|_, s| {
            assert_eq!(
                s[0].content.as_codepoint().map(|c| c.value()),
                Some(u32::from('q')),
                "{arm}: G0 is ASCII again"
            );
        });
        // DECAWM off.
        with(b"\x1b[?7l", &[b'a'; 205], &|_, s| {
            let stride = s.len() / usize::from(ws.ws_row);
            assert_eq!(
                s[stride].content.as_codepoint().map(|c| c.value()),
                Some(u32::from('a')),
                "{arm}: autowrap is on again, so the run wraps onto row 1"
            );
        });
        // DECSCA protection: a selective erase spares a protected cell only.
        with(b"\x1b[1\"q", b"p\x1b[?2J", &|_, s| {
            assert_eq!(
                s[0].content.as_codepoint().map(|c| c.value()),
                Some(0x20),
                "{arm}: protection is off, so the selective erase made the cell a space"
            );
        });
    }
}

/// **`DECSTR` keeps the history, and so does `?1049`.** A full reset empties
/// it; a soft reset is the one a full-screen program sends to recover, and
/// it must not erase what the user had scrolled. With the clear placed in the
/// soft reset -- which the full one calls first -- every `RIS` test stays
/// green; this is the test that says where it belongs.
#[test]
fn a_soft_reset_keeps_the_history() {
    let ws = WinSize {
        ws_col: 60,
        ws_row: 20,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let lines: Vec<u8> = (1..=45)
        .flat_map(|i| format!("{i}\r\n").into_bytes())
        .collect();
    support::feed(&mut t, &lines);
    assert_eq!(t.scrollback().len(), 26, "the control: twenty-six retired");
    support::feed(&mut t, b"\x1b[!p");
    assert_eq!(t.scrollback().len(), 26, "DECSTR kept the history");
    support::feed(&mut t, b"\x1b[?1049h\x1b[!p\x1b[?1049l");
    assert_eq!(
        t.scrollback().len(),
        26,
        "and so did one on the alternate screen"
    );
}

/// The DECRQM answer for each mode after `bytes`: `1` set, `2` reset.
fn modes_after(bytes: &[u8], modes: &[&str]) -> Vec<(String, u8)> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    modes
        .iter()
        .map(|m| {
            t.clear_reply();
            support::feed(&mut t, format!("\x1b[{m}$p").as_bytes());
            let r = String::from_utf8(t.reply().to_vec()).unwrap();
            let v = r
                .strip_prefix(&format!("\x1b[{m};"))
                .and_then(|r| r.strip_suffix("$y"))
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("{m}: {r:?}"));
            (m.to_string(), v)
        })
        .collect()
}

/// **`DECSTR` restores the modes xterm's soft reset restores, and leaves the
/// rest.** `ReallyReset` with `full` false, in the pinned `xterm-charproc.c`:
/// DECTCEM on, DECLRMM off (`resetMarginMode`), DECOM off, DECCKM, DECAWM and
/// reverse wraparound to their power-on values, IRM off. Everything else in
/// the mode word is in its `if (full)` branch or nowhere: LNM, `?4`, `?5`,
/// `?8`, `?1004`, `?1034`, `?1036`, `?1039` and `?2004` stay as the program set
/// them. `?2026` has no xterm precedent -- xterm has no synchronised update --
/// and a soft reset is a recovery, so a held frame is released.
#[test]
fn decstr_restores_the_modes_xterm_restores_and_keeps_the_rest() {
    let set = b"\x1b[?1h\x1b[?6h\x1b[4h\x1b[?7l\x1b[?45h\x1b[?69h\x1b[?25l\x1b[?2026h\
                \x1b[20h\x1b[?4h\x1b[?5h\x1b[?8l\x1b[?1004h\x1b[?1034h\x1b[?1036l\
                \x1b[?1039l\x1b[?2004h";
    let restored = ["?1", "?6", "4", "?7", "?45", "?69", "?25", "?2026"];
    let kept = [
        "20", "?4", "?5", "?8", "?1004", "?1034", "?1036", "?1039", "?2004",
    ];
    let power_on = modes_after(b"", &restored);
    let before = modes_after(set, &kept);
    let mut reset = set.to_vec();
    reset.extend_from_slice(b"\x1b[!p");
    assert_eq!(modes_after(&reset, &restored), power_on, "restored");
    assert_eq!(modes_after(&reset, &kept), before, "kept");
    // The control: every mode here was moved off its power-on value, so each
    // assertion above can fail.
    assert_ne!(modes_after(set, &restored), power_on);
    assert_ne!(before, modes_after(b"", &kept));
}

/// **`DECSTR` puts the keypad back to numeric and `modifyOtherKeys` back to
/// its first value**, as xterm's soft and full resets both do -- in
/// `ReallyReset`'s common path, `UIntClr(..., MODE_DECKPAM)` and
/// `modify_now = modify_1st` -- and as `[[source: vt510-decstr]]` gives DECNKM:
/// "Numeric characters". `RIS` does both too. At level 2, shift-Tab is
/// `CSI 27 ; 2 ; 9 ~`; back at its first value it is `CSI Z`.
#[test]
fn decstr_and_ris_restore_the_keypad_and_modify_other_keys() {
    let tab = KeyEvent::press(KeyCode::Tab, Modifiers::SHIFT);
    for (reset, name) in [(&b"\x1b[!p"[..], "DECSTR"), (&b"\x1bc"[..], "RIS")] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, b"\x1b=\x1b[>4;2m");
        assert_eq!(t.keypad(), KeypadMode::Application, "the control");
        t.clear_reply();
        t.key(tab);
        assert_eq!(t.reply(), b"\x1b[27;2;9~", "the control: level 2");
        support::feed(&mut t, reset);
        assert_eq!(t.keypad(), KeypadMode::Numeric, "{name}: the keypad");
        t.clear_reply();
        t.key(tab);
        assert_eq!(t.reply(), b"\x1b[Z", "{name}: modifyOtherKeys");
    }
}

// ── the cursor ──────────────────────────────────────────────────────────────

/// 20 by 8, for the cursor tests below.
const SMALL: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// The replies `bytes` draw and the cell `X` lands in, 0-based, after
/// requiring the same of every read size and every split into two reads.
fn landed(bytes: &[u8]) -> (String, Option<(usize, usize)>) {
    let at = |pieces: &[&[u8]]| {
        let mut s = Owned::new(&SMALL);
        let mut t = Term::new(s.storage(), &SMALL);
        let mut reply = Vec::new();
        for piece in pieces {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
            reply.extend_from_slice(t.reply());
            t.clear_reply();
        }
        let x = (0..usize::from(SMALL.ws_row)).find_map(|y| {
            t.row(Row::from_index(y))
                .expect("a row on the grid")
                .iter()
                .position(|c| c.content.as_codepoint().map(|cp| cp.value()) == Some(u32::from('X')))
                .map(|x| (y, x))
        });
        (String::from_utf8_lossy(&reply).into_owned(), x)
    };
    let whole = at(&[bytes]);
    for read in 1..bytes.len() {
        let pieces: Vec<&[u8]> = bytes.chunks(read).collect();
        assert_eq!(at(&pieces), whole, "{read}-byte reads of {bytes:02x?}");
    }
    for cut in 1..bytes.len() {
        let (a, b) = bytes.split_at(cut);
        assert_eq!(at(&[a, b]), whole, "split at {cut} of {bytes:02x?}");
    }
    whole
}

/// **`DECSTR` leaves the cursor where it is.** `[[source: dec-std-070]]`
/// 070-4, DECSTR note c (p. 32, PDF 236): "DECSTR has no affect on the
/// Active Position", and its `soft_terminal_reset` (p. 34, PDF 238)
/// assigns a position only to the cursor save buffer. The VT510's table
/// (`[[source: vt510-decstr]]`) and the VT220's Table 4-10
/// (`[[source: vt220-rm-ch4]]`) give "Home" for the saved cursor state
/// alone, and xterm's soft branch zeroes `sc[].row` and `sc[].col` and moves
/// nothing (`[[source: xterm-reset]]`). baud homed it until 2026-10-14.
#[test]
fn decstr_leaves_the_cursor_where_it_is() {
    assert_eq!(
        landed(b"\x1b[5;9H\x1b[!p\x1b[6nX"),
        ("\x1b[5;9R".into(), Some((4, 8)))
    );
}

/// **And a pending wrap at the screen's edge survives it**: STD 070's
/// algorithm does not touch the Last Column Flag either, and xterm's soft
/// branch leaves `do_wrap`. Whether a pending wrap survives `DECSTR` is
/// open (note V, "PendingAt"); this is the option the text leans to. `CPR`
/// reports the last column, and `X` wraps to the next row.
#[test]
fn decstr_keeps_a_pending_wrap_at_the_screen_edge() {
    let mut bytes = b"\x1b[2;1H".to_vec();
    bytes.extend_from_slice(&[b'a'; 20]);
    bytes.extend_from_slice(b"\x1b[!p\x1b[6nX");
    assert_eq!(landed(&bytes), ("\x1b[2;20R".into(), Some((2, 0))));
}

/// **Under DECOM the same**: margins 2..6 and 5..10, origin mode set and
/// `CUP 2;3` from the margins' corner, which is line 3, column 7; `DECSTR`
/// resets the mode and the margins and leaves the cursor there, so `CPR`
/// answers 3;7 from the screen's corner. `tests/vt420/origin.rs` holds the
/// same at every read size, through its own fixture.
#[test]
fn decstr_under_origin_mode_leaves_the_cursor_at_every_split() {
    assert_eq!(
        landed(b"\x1b[2;6r\x1b[?69h\x1b[5;10s\x1b[?6h\x1b[2;3H\x1b[!p\x1b[6nX"),
        ("\x1b[3;7R".into(), Some((2, 6)))
    );
}

/// **At a right margin narrower than the screen the column stays where the
/// pending-wrap sentinel put it**, one past the margin, which the widened
/// margins then read as a column of its own: the wrap is lost and `X` lands
/// one right of the margin. The sentinel cannot tell this from a cursor
/// `CUP` put past the fence, where leaving it is right; `?69 l` does the
/// same. Open, with the options in note V's "PendingAt".
#[test]
fn decstr_leaves_a_column_past_a_narrower_right_margin() {
    assert_eq!(
        landed(b"\x1b[?69h\x1b[5;10s\x1b[3;5Habcdef\x1b[!p\x1b[6nX"),
        ("\x1b[3;11R".into(), Some((2, 10)))
    );
}
