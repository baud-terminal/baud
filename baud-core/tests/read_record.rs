//! **The core's record of each read** (`Term::take_record`, `src/record.rs`):
//! what a read did off the path a read of text and line feeds takes, said by
//! the core that ran it rather than guessed from the bytes.
//!
//! Each test feeds one read, takes its record, and checks one thing it says,
//! with a control that the same read without the thing says nothing of it.
//! Named breaks are given per test: the change that turns it red.
#![cfg(feature = "read-record")]
mod support;
use baud_core::*;
use baud_domain::{BytePos, LineIdx, ReadLen, Rows, WinSize};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 10,
};

/// One read of `bytes`, whole.
fn read(t: &mut Term<'_>, bytes: &[u8]) -> ReadRecord {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    t.take_record()
}

/// The record of `bytes` read after `before` was read and its record taken,
/// on a fresh terminal of `ws`.
fn after(ws: &WinSize, before: &[u8], bytes: &[u8]) -> ReadRecord {
    support::with_term(ws, |t| {
        if !before.is_empty() {
            let _ = read(t, before);
        }
        read(t, bytes)
    })
}

/// The primary's line for row `y`, where no line has scrolled away.
fn line(y: usize) -> LineIdx {
    LineIdx::FIRST.plus(Rows::of_count(y))
}

/// The span of the primary's rows `a` to `b`.
fn rows(a: usize, b: usize) -> Option<Span> {
    Some(Span::of(line(a), line(b)))
}

/// **Text and line feeds leave the record empty**: it begins and ends where
/// the cursor did, nothing off its path, under UTF-8, at ground both ends,
/// every line feed moved. Break: record every row change (`set_row`
/// recording without the climb test) -- the forward read is not empty.
#[test]
fn a_read_of_text_and_line_feeds_records_nothing_off_its_path() {
    let r = after(&WS, b"", b"one\r\ntwo\r\n\x1b[1mthree\x1b[0m");
    assert_eq!(r.start().line, line(0));
    assert_eq!(r.end().line, line(2));
    assert_eq!(r.off_path(Screen::Normal), None);
    assert_eq!(r.off_path(Screen::Alternate), None);
    assert_eq!(r.encoding(), ReadEncoding::Utf8);
    assert_eq!(r.grounded(), (Grounded::Ansi, Grounded::Ansi));
    assert_eq!(r.feeds(), Feeds::Moved);
    assert_eq!(r.moves().iter().count(), 0);
    assert_eq!(r.renumbered(), None);
}

/// **A climb records the rows from where the cursor lands to the lowest it
/// stood on** -- E5's `sank`: a path written a row below and the cursor
/// moved back up leaves that row in the record although the read ends above
/// it. Control: the same read without the climb records nothing. Break: no
/// record at the climb.
#[test]
fn a_climb_records_down_to_the_lowest_row_the_cursor_reached() {
    let down = after(&WS, b"$ ", b"\r\n    /etc/passwd\r");
    assert_eq!(down.off_path(Screen::Normal), None, "the control");
    let back = after(&WS, b"$ ", b"\r\n    /etc/passwd\x1b[A\r");
    assert_eq!(back.off_path(Screen::Normal), rows(0, 1));
    assert_eq!(back.end().line, line(0), "ended above the row it wrote");
}

/// **A line feed ending a read under text it moved up through keeps the
/// line**: text on a row, `CR`, `CSI A`, `LF` ends where it began, the row
/// in the record. Break: no record at the climb.
#[test]
fn text_then_a_move_up_and_a_line_feed_keeps_the_row() {
    let r = after(&WS, b"\r\n", b"/etc/hosts\r\x1b[A\n");
    assert_eq!(r.off_path(Screen::Normal), rows(0, 1));
}

/// **A byte run under ISO 8859-1 makes the read `Other`, even when it
/// switches back before it ends.** Control: a read under UTF-8 throughout.
/// Break: read the encoding only at the read's ends (`take_record`), not at
/// the switch (`Cold::set_input_encoding`).
#[test]
fn latin1_then_back_to_utf8_in_one_read_is_other() {
    let utf8 = after(&WS, b"", b"plain");
    assert_eq!(utf8.encoding(), ReadEncoding::Utf8, "the control");
    let both = after(&WS, b"", b"\x1b%@x\x9d7;file:///tmp\x9c\x1b%G");
    assert_eq!(both.encoding(), ReadEncoding::Other);
    let next = support::with_term(&WS, |t| {
        let _ = read(t, b"\x1b%@");
        read(t, b"x")
    });
    assert_eq!(
        next.encoding(),
        ReadEncoding::Other,
        "begun under ISO 8859-1"
    );
}

/// **Ground is the parser's ground or a control's aftermath**: a read ending
/// in a line feed ends at ground (`AfterControl`), one ending inside a
/// `CSI` does not, and the next read begins inside it. Break: ground as
/// `Node::Ground` alone -- the line feed's read is `Inside`.
#[test]
fn a_line_feed_leaves_the_parser_at_ground_and_a_split_csi_does_not() {
    let lf = after(&WS, b"", b"text\r\n");
    assert_eq!(lf.grounded(), (Grounded::Ansi, Grounded::Ansi));
    support::with_term(&WS, |t| {
        let split = read(t, b"x\x1b[");
        assert_eq!(split.grounded(), (Grounded::Ansi, Grounded::Inside));
        let rest = read(t, b"5A");
        assert_eq!(rest.grounded(), (Grounded::Inside, Grounded::Ansi));
        assert_eq!(rest.off_path(Screen::Normal), None, "row 0 to row 0");
    });
}

/// **A line feed on the last row below a region that ends higher does not
/// move, and the record says so** (R1). Control: one row higher, it moves.
/// Break: no record in `Paint::index`'s no-op branch.
#[test]
fn a_line_feed_that_cannot_move_is_recorded_stuck() {
    let moved = after(&WS, b"\x1b[1;5r\x1b[8;1H", b"\n/etc/passwd\r\n");
    assert_eq!(moved.feeds(), Feeds::Moved, "the control");
    let stuck = after(&WS, b"\x1b[1;5r\x1b[10;1H", b"/etc/passwd\r\n");
    assert_eq!(stuck.feeds(), Feeds::Stuck);
}

/// **An erase off the cursor's row is recorded, one on it is not**: `ED 1`
/// from row 5 reaches rows 0 to 5; `EL` reaches only the cursor's row.
/// Break: no record in `Paint::edit_reaches`.
#[test]
fn an_erase_above_the_cursor_is_recorded_and_one_on_its_row_is_not() {
    let el = after(&WS, b"\x1b[6;1H", b"\x1b[K");
    assert_eq!(el.off_path(Screen::Normal), None, "the control");
    let ed = after(&WS, b"\x1b[6;1H", b"\x1b[1J");
    assert_eq!(ed.off_path(Screen::Normal), rows(0, 5));
}

/// **`IL` records the text it moves down**, exactly: rows 1 to 8 to rows 2
/// to 9, and the row it blanks. Break: no record in `Cold::lines_inserted`.
#[test]
fn an_insert_line_records_the_text_it_moves() {
    let r = after(&WS, b"\x1b[2;1H", b"\x1b[L");
    let moves: Vec<Move> = r.moves().iter().collect();
    assert_eq!(
        moves,
        [Move::Exact {
            screen: Screen::Normal,
            from: Span::of(line(1), line(8)),
            by: LineShift::Down(Rows::of_count(1)),
        }]
    );
    assert!(r.off_path(Screen::Normal).is_some_and(|s| s.holds(line(1))));
}

/// **A region anchored at the top retires its lines and the rows under it
/// take new numbers**, recorded as one move however many lines it scrolls
/// (M1): fifty line feeds at the bottom of `DECSTBM 1 ; 9` fold to the
/// status row's text moved fifty lines down the numbering. Break: no
/// folding (`Move::then`) -- eight moves and a hull.
#[test]
fn a_top_region_scrolled_fifty_times_is_one_move() {
    let r = after(&WS, b"\x1b[1;9r\x1b[9;1H", &b"x\n".repeat(50));
    let moves: Vec<Move> = r.moves().iter().collect();
    assert_eq!(
        moves,
        [Move::Exact {
            screen: Screen::Normal,
            from: Span::of(line(9), line(9)),
            by: LineShift::Down(Rows::of_count(50)),
        }]
    );
}

/// **A region not anchored at the top, scrolled line by line, folds to a
/// move within its band**: its text moved among its own lines. Break: no
/// folding -- eight moves and a hull.
#[test]
fn a_region_scrolled_line_by_line_moves_within_its_band() {
    let r = after(&WS, b"\x1b[3;8r\x1b[8;1H", &b"x\n".repeat(20));
    let moves: Vec<Move> = r.moves().iter().collect();
    assert_eq!(
        moves,
        [Move::Within {
            screen: Screen::Normal,
            band: Span::of(line(2), line(7)),
        }]
    );
}

/// **`?1049` clears the alternate screen and the record says so**; the line
/// the cursor left on the primary, and the one it came back to, are the
/// read's there. `?47` clears nothing. Break: no `Cleared` at `?1049 h`;
/// clearing at `?47 h`.
#[test]
fn the_alternate_screen_cleared_on_entry_and_not_under_47() {
    let r = after(&WS, b"\x1b[4;1H", b"\x1b[?1049hfull\r\nscreen\x1b[?1049l");
    assert_eq!(r.cleared(), Cleared::Alternate);
    assert_eq!(r.off_path(Screen::Normal), rows(3, 3));
    let bare = after(&WS, b"\x1b[4;1H", b"\x1b[?47hkept\x1b[?47l");
    assert_eq!(bare.cleared(), Cleared::No);
}

/// **A folder report and a prompt mark are recorded where their strings
/// end in the read**, with what the core did. Break: no record in
/// `Paint::report_directory`; none in the `OSC 133` arm.
#[test]
fn a_folder_report_and_a_mark_are_recorded_where_they_end() {
    let osc7 = b"\x1b]7;file:///tmp\x07";
    let mark = b"\x1b]133;D;0\x07";
    let bytes = [b"ab".as_slice(), osc7, mark].concat();
    let r = after(&WS, b"", &bytes);
    let folders: Vec<FolderEvent> = r.folders().iter().collect();
    assert_eq!(
        folders,
        [FolderEvent {
            at: BytePos::from_index(2 + osc7.len() - 1),
            folder: Folder::Kept,
        }]
    );
    let marks: Vec<MarkEvent> = r.marks().iter().collect();
    assert_eq!(
        marks,
        [MarkEvent {
            at: BytePos::from_index(bytes.len() - 1),
            letter: b'D',
        }]
    );
}

/// **A change of columns renumbers the primary's lines, a change of height
/// does not** (M2). Break: renumber on every resize.
#[test]
fn a_change_of_columns_renumbers_and_one_of_height_does_not() {
    support::with_room(20, |t| {
        let _ = read(t, b"text\r\n");
        let (cols, _) = t.geometry();
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).expect("cols"),
            ws_row: 15,
        });
        assert_eq!(t.take_record().renumbered(), None, "height alone");
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index() - 10).expect("cols"),
            ws_row: 15,
        });
        assert_eq!(t.take_record().renumbered(), Some(Renumbered::Resize));
    });
}

/// **A span is its lines, either way round, and two spans' hull is both
/// and the lines between.** Break: a hull of the first span alone.
#[test]
fn a_span_and_the_hull_of_two() {
    let a = Span::of(line(5), line(2));
    assert_eq!((a.first(), a.last()), (line(2), line(5)));
    let h = a.hull(Span::of(line(8), line(9)));
    assert_eq!((h.first(), h.last()), (line(2), line(9)));
    assert!(h.holds(line(7)) && !h.holds(line(1)));
}

/// **What the differential found, pinned** (`read_record_props.rs`): each a
/// write the record had missed. `RIS` clears every row; DECCARA changes the
/// rendition of rows above the cursor; `CSI ? 3 J` erases the history's
/// lines in place; a scroll down takes a history line back onto the
/// cursor's row and blanks it; a prompt redraw erases the block's lines up
/// to the cursor. Breaks: drop each record (`hard_reset`,
/// `change_area_attrs`, `selective_erase_saved_lines`,
/// `scroll_region_down`, `replace_prompt_block`).
#[test]
fn the_writes_the_differential_found_are_recorded() {
    let ris = after(&WS, b"\x1b#8\x1b[3;1H", b"\x1bc");
    assert_eq!(ris.off_path(Screen::Normal), rows(0, 9), "RIS");
    let deccara = after(&WS, b"\x1b#8\x1b[5;1H", b"\x1b[1;1;2;5;1$r");
    assert_eq!(deccara.off_path(Screen::Normal), rows(0, 1), "DECCARA");
    support::with_term(&WS, |t| {
        let _ = read(t, b"\x1b#8\x1b[4S");
        let erased = read(t, b"\x1b[?3J");
        let history = erased.off_path(Screen::Normal).expect("the history");
        assert!(
            history.holds(line(0)) && history.holds(line(3)),
            "?3J {history:?}"
        );
    });
    support::with_term(&WS, |t| {
        let _ = read(t, b"\x1b#8\x1b[1S\x1b[1;1H");
        let down = read(t, b"\x1b[T");
        assert!(
            down.off_path(Screen::Normal)
                .is_some_and(|s| s.holds(line(0))),
            "SD {down:?}"
        );
    });
    support::with_term(&WS, |t| {
        // A block taller than the screen: its first three lines are behind
        // the window, and the cursor's row is not blank, so they are erased
        // where they lie.
        let _ = read(
            t,
            b"\x1b]133;A\x07$ 0\r\n1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9\r\n10\r\n11\r\n12",
        );
        let redraw = read(t, b"\x1b]133;A;redraw=1\x07");
        assert!(
            redraw
                .off_path(Screen::Normal)
                .is_some_and(|s| s.holds(line(0))),
            "redraw {redraw:?}"
        );
    });
}

/// **The status display is no line of either screen's**: what a read does
/// on it names nothing, while what it does around it still does. A scroll
/// down, a scroll up, both again under side margins, a rectangle copied and
/// lines inserted and deleted on the one-row display, from a primary with
/// history, leave the record empty; the same scroll down on the primary
/// names its top line, and the side-margin scroll on the alternate screen's
/// rows a band it moved text within. Breaks: drop the `on_status` gate in
/// `Paint::record_rows`, or the line's branch in `Paint::scroll_rect`, which
/// keeps the side-margin scroll from `Cold::moved_in_place`.
#[test]
fn what_a_read_does_on_the_status_display_names_no_line() {
    let ops: [&[u8]; 7] = [
        b"\x1b[T",
        b"\x1b[S",
        b"\x1b[?69h\x1b[2;5s\x1b[S",
        b"\x1b[?69h\x1b[2;5s\x1b[T",
        b"\x1b[1;1;1;5;1;1;3;1$v",
        b"\x1b[L",
        b"\x1b[M",
    ];
    for op in ops {
        support::with_term(&WS, |t| {
            let _ = read(t, b"\x1b#8\x1b[3S\x1b[2$~\x1b[1$}status");
            let on = read(t, op);
            assert_eq!(on.off_path(Screen::Normal), None, "{op:?} {on:?}");
            assert_eq!(on.off_path(Screen::Alternate), None, "{op:?} {on:?}");
            assert_eq!(on.moves().iter().count(), 0, "{op:?} {on:?}");
        });
    }
    // The side-margin scroll, on the alternate screen's own rows, moves
    // text within its band, and on the line it names nothing (above).
    let within = after(&WS, b"\x1b[?1049h\x1b#8", b"\x1b[?69h\x1b[2;5s\x1b[S");
    assert!(
        within.moves().iter().any(|m| matches!(
            m,
            Move::Within {
                screen: Screen::Alternate,
                ..
            }
        )),
        "{within:?}"
    );
    let main = after(&WS, b"\x1b#8\x1b[3S\x1b[1;1H", b"\x1b[T");
    assert!(
        main.off_path(Screen::Normal)
            .is_some_and(|s| s.holds(line(3))),
        "{main:?}"
    );
}

/// **A scroll on the status display leaves the alternate screen's count
/// where it was**: the line reads as the alternate screen while it is
/// written, and its scrolls reach the alternate screen's retire. Lines
/// scrolled up and down on the line, from the alternate screen, and a
/// rendition then changed on its top two rows name the same lines as with
/// no scroll on the line. And `RIS` from the line starts the
/// count again as a fresh terminal's. Breaks: drop the swap of `alt_top`
/// in `Paint::swap_display`, or the parked copy's reset in
/// `Paint::alt_cleared`.
#[test]
fn a_scroll_on_the_status_display_keeps_the_alternate_count() {
    let deccara = b"\x1b[6;1H\x1b[1;1;2;5;1$r";
    let named = |status: &[u8]| {
        support::with_term(&WS, |t| {
            let _ = read(t, b"\x1b[?1049h\x1b#8\x1b[2$~");
            let _ = read(t, status);
            read(t, deccara).off_path(Screen::Alternate)
        })
    };
    let control = named(b"\x1b[1$}\x1b[0$}");
    assert!(control.is_some(), "{control:?}");
    for scroll in [
        &b"\x1b[1$}\x1b[3S\x1b[0$}"[..],
        b"\x1b[1$}\x1b[2T\x1b[0$}",
        b"\x1b[1$}\n\n\n\x1b[0$}",
    ] {
        assert_eq!(named(scroll), control, "{scroll:?}");
    }
    // `RIS` from the line starts the alternate screen's count again, the
    // copy that waits parked as well as the line's.
    let after_ris = support::with_term(&WS, |t| {
        let _ = read(t, b"\x1b[?1049h\x1b[3S\x1b[?1049l\x1b[2$~\x1b[1$}");
        let _ = read(t, b"\x1bc");
        read(t, b"\x1b[?47h\x1b#8\x1b[6;1H\x1b[1;1;2;5;1$r").off_path(Screen::Alternate)
    });
    let fresh = support::with_term(&WS, |t| {
        read(t, b"\x1b[?47h\x1b#8\x1b[6;1H\x1b[1;1;2;5;1$r").off_path(Screen::Alternate)
    });
    assert_eq!(after_ris, fresh);
}

/// **Where a line's text came from, by the moves undone**
/// (`ReadRecord::carried_from`): `IL` at row 3 and then `DL 2` at the top
/// take line 5's text to line 6 and then to line 4, so line 4's sources
/// hold line 5 and not line 7; a side-margin scroll moves text within its
/// band, so any line of it may have come from any other. Breaks: drop the
/// exact move's span, or the band, from `carried_from`.
#[test]
fn a_line_s_sources_are_the_moves_undone() {
    let rec = after(&WS, b"\x1b#8", b"\x1b[4;1H\x1b[L\x1b[1;1H\x1b[2M");
    let sources = rec.carried_from(Screen::Normal, line(4));
    assert!(
        sources.holds(line(5)) && sources.holds(line(4)),
        "{sources:?} {rec:?}"
    );
    assert!(!sources.holds(line(7)), "{sources:?}");
    assert!(sources.iter().count() >= 2, "{sources:?}");
    let within = after(
        &WS,
        b"\x1b[?1049h\x1b#8",
        b"\x1b[?69h\x1b[2;5s\x1b[3;8r\x1b[S",
    );
    let band = within.moves().iter().find_map(|m| match m {
        Move::Within { band, .. } => Some(band),
        Move::Exact { .. } => None,
    });
    let band = band.expect("a band");
    let sources = within.carried_from(Screen::Alternate, band.first());
    assert!(sources.holds(band.last()), "{sources:?} {within:?}");
}

/// **`CSI ? 69 h` makes every double-width line single** (DECLRMM,
/// `Paint::single_width_pages`): a line's rendition written off the
/// cursor's row, so the read names it, and a read that sets the mode with
/// no double-width line names nothing off its path. Break: drop the
/// `wrote_rows` in `single_width_pages`.
#[test]
fn declrmm_names_the_double_width_lines_it_makes_single() {
    let set = after(&WS, b"\x1b[3;1H\x1b#6ab\x1b[6;1H", b"\x1b[?69h");
    assert!(
        set.off_path(Screen::Normal)
            .is_some_and(|s| s.holds(line(2))),
        "{set:?}"
    );
    let plain = after(&WS, b"\x1b[3;1Hab\x1b[6;1H", b"\x1b[?69h");
    assert_eq!(plain.off_path(Screen::Normal), None, "{plain:?}");
}

/// **After an APC G left open, the record's marks are the ones the core
/// acted on** (mpv's exit leaves `ESC _ G a=d;` with no ST): a `133;D`
/// after it, ended by BEL or ST, fed whole, cut at every byte into two
/// reads, and a byte a read, is in the records exactly as often as the
/// core acted on a prompt mark -- whatever the core reads of the string,
/// so whichever reading of an APC G it has. The control: the same mark
/// after a closed APC G is acted on once, and recorded once. Break: record
/// a mark from the bytes rather than from the core's dispatch of it.
#[test]
fn after_an_open_apc_g_the_marks_are_the_ones_the_core_acted_on() {
    let marks_of = |reads: &[&[u8]]| {
        support::with_term(&WS, |t| {
            let before = t.semantic_marks;
            let recorded: usize = reads
                .iter()
                .map(|r| read(t, r).marks().iter().count())
                .sum();
            (recorded, t.semantic_marks - before)
        })
    };
    for end in [&b"\x07"[..], b"\x1b\\"] {
        let open = [b"\x1b_Ga=d;\x1b]133;D".as_slice(), end].concat();
        let closed = [b"\x1b_Ga=d\x1b\\\x1b]133;D".as_slice(), end].concat();
        assert_eq!(marks_of(&[&closed]), (1, 1), "closed {end:?}");
        let whole = marks_of(&[&open]);
        assert_eq!(whole.0, whole.1, "whole {end:?}");
        for cut in 1..open.len() {
            let (a, b) = open.split_at(cut);
            let (recorded, acted) = marks_of(&[a, b]);
            assert_eq!(recorded, acted, "cut at {cut} {end:?}");
            assert_eq!(
                (recorded, acted),
                whole,
                "cut at {cut} {end:?} reads as whole"
            );
        }
        let bytes: Vec<&[u8]> = open.chunks(1).collect();
        assert_eq!(marks_of(&bytes), whole, "a byte a read {end:?}");
    }
}
