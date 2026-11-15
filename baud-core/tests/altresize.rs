//! Growing the window must not move what is already on the screen.
//!
//! A user's report, twice: *"when resizing the current first row changes
//! and it should not -- we should not change the scroll ever, we should
//! only re-arrange the grid of what is visible, not go to history."* A
//! screen recording of btop mid-drag shows what it costs -- three copies
//! of its lower panels at three different sizes, stacked down the
//! screen, because each redraw landed where the last one had been moved
//! to rather than over it.
mod support;
use baud_core::{PackedCell, Term};
use baud_domain::WinSize;

/// The first eight cells of screen row `y`, as text.
fn text(t: &Term<'_>, y: usize) -> String {
    t.row(baud_domain::Row::from_index(y))
        .expect("a row on the grid")[..8]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect()
}

/// The first `rows` screen rows, raw, so a comparison is cell for cell.
fn rows_of(t: &Term<'_>, rows: usize) -> Vec<Vec<PackedCell>> {
    let cells = support::screen(t);
    let stride = t.stride().index();
    let cols = t.geometry().0.index();
    (0..rows)
        .map(|y| cells[y * stride..y * stride + cols].to_vec())
        .collect()
}

#[test]
fn a_taller_alternate_screen_does_not_move_what_is_on_it() {
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[?1049h");
        let (_, rows) = t.geometry();
        // A full-screen application's frame: every row addressed
        // absolutely, which is how btop and asciiquarium draw.
        for y in 0..rows.index() {
            support::feed(t, format!("\x1b[{};1Hrow {y:03}", y + 1).as_bytes());
        }
        let before = rows_of(t, 8);
        let (cols, _) = t.geometry();
        assert_eq!(
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok,
            "the growth is inside the provisioned grid"
        );
        let after = rows_of(t, 8);
        for (y, (b, a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(
                b, a,
                "row {y} moved when the window grew: the screen scrolled under the application"
            );
        }
    });
}

/// **The primary screen is the other way round, and this test used to say
/// otherwise.** It was written on 2026-09-14 from the report quoted above,
/// as "a taller primary screen does not pull history back", and it could not
/// fail: `with_corpus` provisions exactly the fifty rows the screen has, so
/// the growth came back `Truncated` into a `let _`, nothing grew, and the
/// comparison held on a screen that had not moved. The rule for the primary
/// is "pull down to the bottom, and blank-fill only when nothing is left to
/// pull" (`Term::resize`), because refusing to pull loses the lines a shrink
/// pushed into the history. The
/// alternate screen keeps no history and has nothing to pull, which is why
/// the report above still governs it.
///
/// So this pins the rule in force, on a path most growth tests do not take:
/// the lines pulled back are ones that **scrolled** off, not ones a shrink
/// retired (two pins in `scrollback.rs` pull scrolled lines too). The bottom
/// row stays the bottom row, the rows above it are the history in order, and
/// the history is shorter by what came back.
#[test]
fn a_taller_primary_screen_pulls_the_scrolled_lines_back_above_it() {
    support::with_room(60, |t| {
        // Sixty numbered lines and the cursor's blank row below them: 61
        // rows on 50, so eleven have scrolled off.
        for i in 0..60 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        assert_eq!(t.scrollback().len(), 11, "lines 000 to 010 scrolled off");
        assert_eq!(text(t, 0), "line 011");
        let (cols, rows) = t.geometry();
        assert_eq!(
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok,
            "the growth is inside the provisioned grid"
        );
        for y in 0..6 {
            assert_eq!(
                text(t, y),
                format!("line {:03}", y + 5),
                "pulled back, in order"
            );
        }
        assert_eq!(
            text(t, 6),
            "line 011",
            "what was the first row is below them"
        );
        assert_eq!(text(t, 54), "line 059");
        assert_eq!(
            t.cursor().0.index(),
            55,
            "the cursor's row stays the bottom row"
        );
        assert_eq!(t.scrollback().len(), 5, "and the history is shorter by six");
    });
}

/// **A taller alternate screen reveals blank rows, not the lines it scrolled
/// off its top.** The alternate screen keeps no history, but its ring is the
/// provisioned height, so a line that scrolls off the top stays in the ring
/// behind the window. A growth moves the window's bottom edge over those
/// rows, and nothing blanked them: the lines a full-screen program scrolled
/// away came back under its last frame until it repainted. A wider alternate
/// screen here already blanks the columns it reveals (`View::widened`) for
/// the same reason, and this is the height half of that: a rule of baud's
/// own, since what xterm does to its alternate buffer on a resize is in
/// `screen.c`, which is not pinned.
#[test]
fn a_taller_alternate_screen_reveals_blank_rows_not_scrolled_ones() {
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[?1049h");
        let (cols, rows) = t.geometry();
        // Sixty lines on a fifty-row screen: the alternate screen scrolls,
        // and the rows that left its top are still in its ring.
        for i in 0..60 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        assert_eq!(
            text(t, 0),
            "line 011",
            "the control: the screen scrolled, so rows are behind the window"
        );
        assert_eq!(
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok,
            "the growth is inside the provisioned grid"
        );
        assert_eq!(text(t, 0), "line 011", "the top row did not move");
        for y in rows.index()..rows.index() + 6 {
            assert_eq!(
                text(t, y),
                "        ",
                "row {y} is new to the screen and must be blank"
            );
        }
    });
}

/// **The same for rows a shorter screen cut off**, which are the other thing
/// left in the ring past the window. A full-screen frame shrunk by six rows
/// and grown back must not show its old bottom six under the frame the
/// program draws next.
#[test]
fn a_shrink_then_a_growth_of_the_alternate_screen_shows_no_old_bottom() {
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[?1049h");
        let (cols, rows) = t.geometry();
        for y in 0..rows.index() {
            support::feed(t, format!("\x1b[{};1Hrow {y:03}", y + 1).as_bytes());
        }
        let at = |t: &mut Term<'_>, r: usize| {
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(r).unwrap(),
            })
        };
        assert_eq!(at(t, rows.index() - 6), baud_core::Resized::Ok);
        assert_eq!(at(t, rows.index()), baud_core::Resized::Ok);
        assert_eq!(text(t, 0), "row 000 ", "the top row did not move");
        assert_eq!(
            text(t, rows.index() - 7),
            "row 043 ",
            "the control: kept rows stay"
        );
        for y in rows.index() - 6..rows.index() {
            assert_eq!(
                text(t, y),
                "        ",
                "row {y} was cut by the shrink and must come back blank"
            );
        }
    });
}

/// **A row cut below a shorter alternate screen keeps a mark true only to the
/// width it was cut at, and a width growth has to reach it.** `widened` made
/// the marks of the *screen's* rows true of the wider screen and skipped the
/// rows past the window, so a red erase to the end of a row at 36 columns,
/// cut by a shrink, widened to 50 and revealed by a growth, came back with
/// its red band: the revealed-row blank trusted a mark of 13 and stopped
/// there, and set the mark to zero over the band, so a later default erase
/// took the shortcut and cleared nothing either.
#[test]
fn a_band_on_a_cut_row_is_blanked_when_a_growth_reveals_it() {
    support::with_room(60, |t| {
        let at = |t: &mut Term<'_>, c: u16, r: u16| {
            t.resize(&WinSize {
                ws_col: c,
                ws_row: r,
            })
        };
        assert_eq!(at(t, 36, 50), baud_core::Resized::Ok);
        support::feed(
            t,
            b"\x1b[?1049h\x1b[46;1Habc\x1b[41m\x1b[46;14H\x1b[K\x1b[0m",
        );
        let row = |t: &Term<'_>| {
            t.row(baud_domain::Row::from_index(45))
                .expect("a row on the grid")
                .to_vec()
        };
        assert_ne!(
            row(t)[20],
            PackedCell::EMPTY,
            "the control: the band is red"
        );
        assert_eq!(at(t, 36, 40), baud_core::Resized::Ok, "row 45 is cut");
        assert_eq!(
            at(t, 50, 40),
            baud_core::Resized::Ok,
            "wider while it is cut"
        );
        assert_eq!(at(t, 50, 50), baud_core::Resized::Ok, "and revealed");
        assert!(
            row(t).iter().all(|c| *c == PackedCell::EMPTY),
            "every revealed cell is the default blank"
        );
        support::feed(t, b"\x1b[46;1H\x1b[41mx\x1b[0m\x1b[46;1H\x1b[2K");
        assert!(
            row(t).iter().all(|c| *c == PackedCell::EMPTY),
            "and a later default erase clears the row"
        );
    });
}

/// **A band past the old edge is blanked when a growth reveals it, though
/// the row ends in the default blank.** Erased in red to the end of a row at
/// 36 columns, a row is red to 36 under a mark of zero; narrowed to 30 it
/// keeps the red from 30 to 36 past the edge. Grown to 50, its last cell is
/// the default blank the erase never reached, and `widened` blanked through
/// `fill`, whose shortcut stops at the mark when the last cell is already
/// the blank: it wrote nothing and left the red from 30 to 36 under a mark
/// of 30 that said the row was uniform from there, so a later default erase
/// stopped at 30 too. On the screen, and on a row a shorter screen cut and
/// a growth of both sizes at once reveals.
#[test]
fn a_band_past_the_old_edge_is_blanked_when_a_growth_reveals_it() {
    let at = |t: &mut Term<'_>, c: u16, r: u16| {
        t.resize(&WinSize {
            ws_col: c,
            ws_row: r,
        })
    };
    let row = |t: &Term<'_>, y: usize| {
        t.row(baud_domain::Row::from_index(y))
            .expect("a row on the grid")
            .to_vec()
    };
    support::with_room(60, |t| {
        assert_eq!(at(t, 36, 50), baud_core::Resized::Ok);
        support::feed(t, b"\x1b[?1049h\x1b[6;1H\x1b[41m\x1b[2K\x1b[0m");
        assert_ne!(row(t, 5)[35], PackedCell::EMPTY, "the control: red to 36");
        assert_eq!(at(t, 30, 50), baud_core::Resized::Ok, "narrower");
        assert_eq!(at(t, 50, 50), baud_core::Resized::Ok, "and wider");
        assert!(
            row(t, 5)[30..].iter().all(|c| *c == PackedCell::EMPTY),
            "every revealed cell is the default blank"
        );
        assert_ne!(row(t, 5)[29], PackedCell::EMPTY, "the overlap is kept");
        support::feed(t, b"\x1b[6;1H\x1b[2K");
        assert!(
            row(t, 5).iter().all(|c| *c == PackedCell::EMPTY),
            "and a later default erase clears the row"
        );
    });
    support::with_room(60, |t| {
        assert_eq!(at(t, 36, 50), baud_core::Resized::Ok);
        support::feed(t, b"\x1b[?1049h\x1b[46;1H\x1b[41m\x1b[2K\x1b[0m");
        assert_ne!(row(t, 45)[35], PackedCell::EMPTY, "the control: red to 36");
        assert_eq!(at(t, 30, 40), baud_core::Resized::Ok, "row 45 is cut");
        assert_eq!(at(t, 50, 50), baud_core::Resized::Ok, "and revealed");
        assert!(
            row(t, 45).iter().all(|c| *c == PackedCell::EMPTY),
            "every revealed cell is the default blank"
        );
    });
}

/// **A character the old edge split is blanked whole by the growth.** A
/// wide character at the last two columns of 36, narrowed to 35, has its
/// lead inside the edge and its tail past it. Grown to 50, the revealed
/// columns begin at its tail; blanking from there would leave a lead that
/// claims two columns and owns one, so the blank starts at the lead, as
/// `fill`'s snap to whole characters starts it.
#[test]
fn a_character_the_old_edge_split_is_blanked_whole_by_a_growth() {
    support::with_room(60, |t| {
        let at = |t: &mut Term<'_>, c: u16, r: u16| {
            t.resize(&WinSize {
                ws_col: c,
                ws_row: r,
            })
        };
        // The first row to the pitch, so the tail past the edge is read.
        let row = |t: &Term<'_>| support::screen(t)[..t.stride().index()].to_vec();
        assert_eq!(at(t, 36, 50), baud_core::Resized::Ok);
        support::feed(t, "\x1b[?1049h\x1b[1;35H\u{4e16}".as_bytes());
        assert_eq!(at(t, 35, 50), baud_core::Resized::Ok, "narrower");
        let r = row(t);
        assert!(
            r[34].content.as_codepoint().is_some() && r[35].content.is_wide_tail(),
            "the control: the lead inside the edge, the tail past it"
        );
        assert_eq!(at(t, 50, 50), baud_core::Resized::Ok, "and wider");
        assert!(
            row(t)[34..50].iter().all(|c| *c == PackedCell::EMPTY),
            "the character is blanked whole, its lead with its tail"
        );
    });
}

/// **A growth leaves no tail past the edge for the next one to snap to.** A
/// wide character at columns 39 and 40 of 60, narrowed to 30 and grown to
/// 35, lies wholly past the new edge under a mark of 41. A growth that
/// lowered the mark without clearing to it left the tail at 40 standing;
/// grown to 40 and an `X` written at 39, the growth to 50 snapped its blank
/// back from 40 onto the `X`, because a start on a tail belongs to the lead
/// before it. Cleared to the mark, as a fill to the end of a row clears a
/// row whose mark lies past the edge, there is no tail and the `X` stays.
#[test]
fn a_growth_leaves_no_tail_past_the_edge_for_the_next_one_to_snap_to() {
    let at = |t: &mut Term<'_>, c: u16, r: u16| {
        t.resize(&WinSize {
            ws_col: c,
            ws_row: r,
        })
    };
    // The first row to the pitch, so the cells past the edge are read.
    let row = |t: &Term<'_>| support::screen(t)[..t.stride().index()].to_vec();
    let cp = |c: &PackedCell| c.content.as_codepoint().map(|c| c.value());
    support::with_room(60, |t| {
        assert_eq!(at(t, 60, 50), baud_core::Resized::Ok);
        support::feed(t, "\x1b[?1049h\x1b[1;40H\u{4e16}".as_bytes());
        let r = row(t);
        assert!(
            cp(&r[39]) == Some(0x4e16) && r[40].content.is_wide_tail(),
            "the control: the lead at 39, the tail at 40"
        );
        assert_eq!(at(t, 30, 50), baud_core::Resized::Ok, "narrower");
        assert_eq!(at(t, 35, 50), baud_core::Resized::Ok, "wider");
        assert!(
            !row(t)[40].content.is_wide_tail(),
            "no tail is left past the new edge"
        );
        assert_eq!(at(t, 40, 50), baud_core::Resized::Ok, "wider again");
        support::feed(t, b"\x1b[1;40HX");
        assert_eq!(
            cp(&row(t)[39]),
            Some(u32::from(b'X')),
            "the control: X at 39"
        );
        assert_eq!(at(t, 50, 50), baud_core::Resized::Ok, "and wider");
        assert_eq!(cp(&row(t)[39]), Some(u32::from(b'X')), "the X survives");
    });
}
