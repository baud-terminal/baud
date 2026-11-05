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
        grid_stride(&line128(), Cols::of_index(200)),
        "one function decides the pitch, for the width provisioned"
    );
    assert_eq!(pitch.index(), 208, "13 lines of 128 B for 200 columns");
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
        // The width is provisioned with the pitch: a wider ask is answered
        // `Truncated` to it rather than clamped in silence.
        let want = match usize::from(ws.ws_col) <= pitch.index() {
            true => Resized::Ok,
            false => Resized::Truncated {
                cols: Cols::of_index(pitch.index()),
                rows: Rows::of_index(usize::from(ws.ws_row)),
            },
        };
        assert_eq!(t.resize(&ws), want);
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
    assert_eq!(
        unknown,
        grid_stride(&CacheGeometry::UNKNOWN, Cols::of_index(200))
    );
    assert_eq!(unknown.index(), 200, "25 lines of 64 B for 200 columns");
}

fn rows_text(t: &Term<'_>, rows: usize, n: usize) -> Vec<String> {
    (0..rows)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..n]
                .iter()
                .map(|c| match c.content.as_codepoint() {
                    Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                    None => ' ',
                })
                .collect()
        })
        .collect()
}

/// **A shrink then a growth is the identity.** Sixty numbered lines on a
/// fifty-row screen, so the grid is rotated; shrink to ten by a hundred --
/// the cursor is on the last row, so the forty rows above it retire to the
/// history and every 158-column line is re-cut into two rows, the window
/// sliding down to keep the cursor -- grow back, and the lines join and
/// the rows come back from the history, so every row reads as it did.
/// Read through `row`, which applies the rotation without undoing it.
#[test]
fn a_shrink_then_a_growth_is_the_identity() {
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
        let cut: Vec<String> = (55..60)
            .flat_map(|i| {
                let l = format!("line {i} {}", "x".repeat(150));
                [l[..100].to_string(), format!("{:<100}", &l[100..])]
            })
            .chain([" ".repeat(100)])
            .collect();
        assert_eq!(
            rows_text(t, 10, 100),
            cut[1..],
            "the cursor's row and the nine re-cut rows above it are what is live"
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
        let row0 = t.row(Row::ZERO).expect("a row on the grid");
        assert!(
            row0[..200]
                .iter()
                .all(|c| t.style_of(*c) == StyleWord::DEFAULT),
            "row 0: no red blank survives a default erase"
        );
    });
}

/// A row reused by a scroll while the screen was narrow comes back with the
/// new line and no old tail: the scroll's blank runs through the mark. On
/// the alternate screen, which hides rows rather than retiring them, so the
/// reused row is one that kept a tail.
#[test]
fn a_row_reused_by_a_scroll_while_narrow_comes_back_clean() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h");
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
        // Within the width it was typed at, `new` overwrote a line of `A`s
        // without clearing it, so that row keeps those `A`s -- overwrite
        // semantics. The row the scroll brought in is clean there, which is
        // what a scroll owes: the reused row must not carry the tail of what
        // it replaced. Past the old edge, the grow blanks every row alike --
        // the alternate screen shows the program empty space to redraw into,
        // not the frame it drew when it was last this wide.
        assert!(
            text[8].starts_with("newA"),
            "the new line is on screen: {:?}",
            &text[8][..10]
        );
        assert!(
            text[9].trim().is_empty(),
            "the row the scroll reused carries no tail"
        );
        assert!(
            text[7][..100].ends_with('A'),
            "the row above kept the tail it was typed with"
        );
        assert!(
            text[7][100..].trim().is_empty(),
            "the columns the grow revealed are blank, not the old tail"
        );
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
/// content to be relative to now. From row 39 on a fifty-row screen shrunk
/// to twenty, ten rows below it are cut and twenty above it retire, so it
/// lands on row 19.
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
        // Growing back reveals the ten rows cut below and pulls the twenty
        // retired lines in above, so the cursor's row moves down by twenty.
        assert_eq!(
            t.cursor(),
            (Row::from_index(24), Col::at_index(4)),
            "a cursor inside the new grid moves with its row"
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
    // The pitch is the provisioned width, so provision the width this grows
    // to; the corpus geometry is what the terminal starts at.
    let mut owned = Owned::with_geometry(
        &WinSize {
            ws_row: 50,
            ws_col: 250,
        },
        &CacheGeometry::UNKNOWN,
    );
    let t = &mut Term::new(owned.storage(), &support::CORPUS);
    {
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
        let row = t.row(Row::ZERO).expect("a row on the grid");
        assert!(
            row[..250]
                .iter()
                .all(|c| t.style_of(*c) == StyleWord::DEFAULT),
            "no red survives a default erase over the whole row"
        );
    }
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
        let row = t.row(Row::from_index(2)).expect("a row on the grid");
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

/// **The alternate screen blanks the columns a grow reveals.** A full-screen
/// program owns the alternate screen and redraws a frame or two after a
/// resize; the columns a grow exposes held whatever was there when the screen
/// was last that wide, and showing them under the incoming draw is the "two
/// images at once" a grow used to flash. Fill the alternate screen across all
/// 200 columns, shrink to 100 -- the right half is hidden, not blanked -- and
/// grow back to 200: the revealed columns read blank, while the overlap the
/// program has not yet redrawn keeps its content.
#[test]
fn the_alternate_screen_blanks_the_columns_a_grow_reveals() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h");
        support::feed(t, format!("\x1b[1;1H{}", "X".repeat(200)).as_bytes());
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
        let row = &rows_text(t, 1, 200)[0];
        assert_eq!(&row[..100], &"X".repeat(100), "the overlap is kept");
        assert_eq!(
            &row[100..],
            &" ".repeat(100),
            "the revealed columns are blank, not stale"
        );
    });
}

// **A width growth keeps the marks honest at a prompt too, and a re-cut
// leaves nothing past its width.** The whole width half of `resize` -- the
// primary's re-cut and the alternate screen's `widened` -- sat behind one
// test of the primary's zone, so at a shell prompt a growth maintained no
// marks on either screen; and a narrowing re-cut wrote only the new width
// into its destination rows, so what the rows held before stayed past it.
// Each of the three is shown red first, with the same bytes and no prompt
// mark as its control where one exists.

fn resize_ok(t: &mut Term<'_>, cols: u16) {
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 50,
            ws_col: cols,
        }),
        Resized::Ok,
        "{cols} columns is inside the provisioned grid"
    );
}

/// The template test above, with the shell at a prompt when the screen
/// grows: no re-cut runs, and the marks were not raised either, so the
/// default erase trusted a mark of zero and left the red band.
#[test]
fn at_a_prompt_a_wider_screen_still_erases_a_band_to_the_old_edge() {
    for (mark, arm) in [
        (&b""[..], "the control, no prompt"),
        (&b"\x1b]133;P;k=i\x07"[..], "at a prompt"),
    ] {
        support::with_corpus(|t| {
            resize_ok(t, 100);
            support::feed(t, b"\x1b[3;1H\x1b[41m\x1b[2K\x1b[0m");
            support::feed(t, b"\x1b[50;1H");
            support::feed(t, mark);
            resize_ok(t, 200);
            support::feed(t, b"\x1b[3;1H\x1b[2K");
            let row = t.row(Row::from_index(2)).expect("a row on the grid");
            let red = row[..200]
                .iter()
                .filter(|c| t.style_of(**c) != StyleWord::DEFAULT)
                .count();
            assert_eq!(red, 0, "{arm}: the red band to the old edge is erased");
        });
    }
}

/// The alternate screen grows whatever the shell's zone. A program drawn in
/// a background colour at 100 columns leaves every alternate row red to
/// 100; the shell is at a prompt when the screen grows to 200; the next
/// program's entry clears the alternate screen -- and trusted marks of zero,
/// leaving the band, because `widened` never ran.
#[test]
fn the_alternate_screen_grows_whatever_the_shells_zone() {
    for (mark, arm) in [
        (&b""[..], "the control, no prompt"),
        (&b"\x1b]133;P;k=i\x07"[..], "at a prompt"),
    ] {
        support::with_corpus(|t| {
            resize_ok(t, 100);
            support::feed(t, b"\x1b[?1049h\x1b[41m\x1b[2J\x1b[0m\x1b[?1049l");
            support::feed(t, mark);
            resize_ok(t, 200);
            support::feed(t, b"\x1b[?1049h");
            let red: usize = (0..50)
                .map(|y| {
                    t.row(Row::from_index(y)).expect("a row on the grid")[..200]
                        .iter()
                        .filter(|c| t.style_of(**c) != StyleWord::DEFAULT)
                        .count()
                })
                .sum();
            assert_eq!(
                red, 0,
                "{arm}: the entry cleared the whole alternate screen"
            );
        });
    }
}

/// The same gate, live: a program started from the prompt with no `C` --
/// the shell's zone parked as `Prompt` -- draws a full frame at 200
/// columns, the window narrows to 100 and grows back, and the columns the
/// growth reveals are blank rather than the frame drawn when it was last
/// this wide.
#[test]
fn a_wider_alternate_screen_blanks_what_it_reveals_while_the_shell_is_at_a_prompt() {
    for (mark, arm) in [
        (&b""[..], "the control, no prompt"),
        (&b"\x1b]133;P;k=i\x07"[..], "at a prompt"),
    ] {
        support::with_corpus(|t| {
            support::feed(t, mark);
            support::feed(t, b"\x1b[?1049h");
            for y in 0..50 {
                support::feed(
                    t,
                    format!("\x1b[{};1H{}", y + 1, "#".repeat(200)).as_bytes(),
                );
            }
            resize_ok(t, 100);
            resize_ok(t, 200);
            let stale: usize = (0..50)
                .map(|y| {
                    t.row(Row::from_index(y)).expect("a row on the grid")[100..200]
                        .iter()
                        .filter(|c| c.content.as_codepoint().is_some())
                        .count()
                })
                .sum();
            assert_eq!(stale, 0, "{arm}: the revealed columns are blank");
        });
    }
}

/// A narrowing re-cut writes each line's cells at the new width, and the
/// destination rows held other lines' cells past it. A later growth that
/// showed every column brought those stale tails back beside the re-cut
/// text: the second half of a line with a first half's tail after it. At a
/// prompt the growth re-cuts the output above it, so every row after the
/// round trip is one whole line again, ninety characters, and nothing
/// else.
#[test]
fn a_narrowing_re_cut_leaves_nothing_past_its_width() {
    support::with_corpus(|t| {
        resize_ok(t, 100);
        for i in 0..30 {
            support::feed(t, format!("L{i:02}{}\r\n", "x".repeat(87)).as_bytes());
        }
        resize_ok(t, 50);
        support::feed(t, b"\x1b]133;P;k=i\x07$ ");
        resize_ok(t, 100);
        let long: Vec<(usize, String)> = (0..50)
            .map(|y| {
                let text: String = t.row(Row::from_index(y)).expect("a row on the grid")[..100]
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|p| char::from_u32(p.value()))
                            .unwrap_or(' ')
                    })
                    .collect();
                (y, text.trim_end().to_string())
            })
            .filter(|(_, s)| !s.is_empty() && s != "$")
            .collect();
        let whole = |s: &str| {
            s.len() == 90
                && s.starts_with('L')
                && s[1..3].parse::<u8>().is_ok()
                && s[3..].bytes().all(|b| b == b'x')
        };
        assert!(
            !long.is_empty() && long.iter().all(|(_, s)| whole(s)),
            "every row one whole line: {long:?}"
        );
    });
}

/// **And a growth at a prompt must not lengthen the lines a later re-cut
/// reads.** Raising every mark to the old width -- the rule before the
/// ring -- makes a three-character line read as a hundred cells, and the
/// next narrowing cuts it into rows of blanks. The mark is raised only
/// where the revealed columns are not the row's uniform blank.
#[test]
fn a_growth_at_a_prompt_does_not_lengthen_the_lines_a_later_re_cut_reads() {
    support::with_corpus(|t| {
        resize_ok(t, 100);
        for i in 0..10 {
            support::feed(t, format!("s{i}\r\n").as_bytes());
        }
        support::feed(t, b"\x1b]133;P;k=i\x07$ ");
        resize_ok(t, 200);
        support::feed(t, b"\r\n\x1b]133;C\x07");
        resize_ok(t, 40);
        let rows: Vec<String> = (0..11)
            .map(|y| {
                t.row(Row::from_index(y)).expect("a row on the grid")[..40]
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|p| char::from_u32(p.value()))
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        let want: Vec<String> = (0..10)
            .map(|i| format!("s{i}"))
            .chain(["$".to_string()])
            .collect();
        assert_eq!(
            rows, want,
            "one row a short line, no rows of blanks between"
        );
    });
}

/// **The seam between the primary's two width models, pinned as it is.**
/// At a prompt a narrowing keeps the shell's block whole, and a line typed
/// there keeps its tail hidden past the live edge; once the command runs a
/// later narrowing re-cuts it, and the re-cut reads a row only to the width
/// it had, so the hidden tail is not part of the line any more. It used to
/// stay where it was and come back as stale bytes on the next growth at a
/// prompt; the re-cut clears its rows past the new width now, so it is
/// dropped cleanly. The drop itself is the open question of reading a row
/// held past its width into the line a re-cut cuts, and the count is
/// pinned so it moves when that is decided.
#[test]
fn a_tail_hidden_at_a_prompt_is_dropped_cleanly_by_a_later_re_cut() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b]133;P;k=i\x07$ ");
        support::feed(t, format!("L{}", "x".repeat(190)).as_bytes());
        resize_ok(t, 100);
        support::feed(t, b"\r\n\x1b]133;C\x07");
        resize_ok(t, 50);
        support::feed(t, b"\x1b]133;D\x07\x1b]133;P;k=i\x07$ ");
        resize_ok(t, 200);
        let rows: Vec<String> = (0..50)
            .map(|y| {
                t.row(Row::from_index(y)).expect("a row on the grid")[..200]
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|p| char::from_u32(p.value()))
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        // The growth at the next prompt re-cuts the finished line onto one
        // row: what the narrowing to 50 read of it, its first 100 cells.
        assert_eq!(
            rows[0],
            format!("$ L{}", "x".repeat(97)),
            "the line as the re-cut read it, nothing past it: {rows:?}"
        );
        let x: usize = rows.iter().map(|r| r.matches('x').count()).sum();
        assert_eq!(
            x, 97,
            "the hidden tail's 93 characters are dropped: the open seam"
        );
    });
}
