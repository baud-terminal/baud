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
use baud_domain::{
    BytePos, CommandRuns, LineIdx, MarkBack, MarkLen, ReadLen, Rows, ShellTokenHash, ShellTokenHex,
    TokenSeen, WinSize,
};

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
            replaced: Replaced::Whole,
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
            replaced: Replaced::Whole,
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
    let marks: Vec<(usize, u8)> = r
        .marks()
        .iter()
        .map(|m| (m.at().index(), m.letter().raw_byte()))
        .collect();
    assert_eq!(marks, [(bytes.len() - 1, b'D')]);
}

/// The window's token in these tests, and its hash.
const TOKEN: &[u8; 16] = b"0123456789abcdef";

fn token() -> ShellTokenHash {
    ShellTokenHash::of(&ShellTokenHex::parse(TOKEN).expect("sixteen hex digits"))
}

/// Each mark of `bytes`, read in one read on a terminal holding the
/// window's token when `held`: where it ends, how far back it began, its
/// letter, its token and what it runs.
fn marks_of(
    held: Held,
    before: &[u8],
    bytes: &[u8],
) -> Vec<(usize, MarkBack, u8, TokenSeen, CommandRuns)> {
    support::with_term(&WS, |t| {
        if held == Held::Token {
            t.set_shell_token(Some(token()));
        }
        if !before.is_empty() {
            let _ = read(t, before);
        }
        read(t, bytes)
            .marks()
            .iter()
            .map(|m| {
                (
                    m.at().index(),
                    m.back(),
                    m.letter().raw_byte(),
                    m.token(),
                    m.runs(),
                )
            })
            .collect()
    })
}

/// A mark introduced `n` bytes before its terminator, in the read.
fn within(n: usize) -> MarkBack {
    MarkBack::Within(MarkLen::between(BytePos::ZERO, BytePos::from_index(n)))
}

/// Whether the terminal holds the window's token.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    None,
    Token,
}

/// **A mark is recorded with how far back its introducer is, what it says
/// of the window's token and what a `C` names**, read from the mark as the
/// core dispatched it: the shell's `C` naming `ssh` is `Shells` and
/// `Elsewhere`, from its `ESC` to its `BEL`; a `D` with the token is
/// `Shells`, one with another or none `Other`, and with no token held every
/// mark is `Absent`; a `D` ended by `ESC \` reaches back to its own `ESC`.
/// Named break: read the token against none held (`mark_recorded` passing
/// `None` to `TokenSeen::of`) -- the shell's `C` and `D` read `Absent`.
#[test]
fn a_mark_is_recorded_with_its_introducer_token_and_command() {
    let t = core::str::from_utf8(TOKEN).expect("ascii");
    let c = format!("\x1b]133;C;baud_token={t};cmdline=ssh\\ h\x07");
    let d = format!("\x1b]133;D;0;baud_token={t}\x1b\\");
    let other = "\x1b]133;D;0;baud_token=fedcba9876543210\x07";
    let bare = "\x1b]133;D;0\x07";
    let bytes = [
        b"ab".as_slice(),
        c.as_bytes(),
        d.as_bytes(),
        other.as_bytes(),
        bare.as_bytes(),
    ]
    .concat();
    let ends: Vec<usize> = [c.len(), d.len(), other.len(), bare.len()]
        .iter()
        .scan(2, |at, len| {
            *at += len;
            Some(*at - 1)
        })
        .collect();
    let want = |seen: [TokenSeen; 4]| {
        vec![
            (
                ends[0],
                within(c.len() - 1),
                b'C',
                seen[0],
                CommandRuns::Elsewhere,
            ),
            (
                ends[1],
                within(d.len() - 1),
                b'D',
                seen[1],
                CommandRuns::Nothing,
            ),
            (
                ends[2],
                within(other.len() - 1),
                b'D',
                seen[2],
                CommandRuns::Nothing,
            ),
            (
                ends[3],
                within(bare.len() - 1),
                b'D',
                seen[3],
                CommandRuns::Nothing,
            ),
        ]
    };
    use TokenSeen::{Absent, Other, Shells};
    assert_eq!(
        marks_of(Held::Token, b"", &bytes),
        want([Shells, Shells, Other, Other])
    );
    assert_eq!(marks_of(Held::None, b"", &bytes), want([Absent; 4]));
}

/// **A mark begun in the read before reaches back past the read**
/// (`MarkBack::Before`), and so does one whose `]` is the read's first
/// byte; a C1 introducer under ISO 8859-1 is its own first byte. Named
/// break: keep the introducer's offset across records -- the split mark
/// reads as begun inside the second read.
#[test]
fn a_mark_begun_before_the_read_reaches_back_past_it() {
    let before = MarkBack::Before;
    let split = marks_of(Held::None, b"xy\x1b]133;D;", b"0\x07");
    assert_eq!(
        split,
        [(1, before, b'D', TokenSeen::Absent, CommandRuns::Nothing)]
    );
    let at_bracket = marks_of(Held::None, b"\x1b", b"]133;A\x07");
    assert_eq!(
        at_bracket,
        [(6, before, b'A', TokenSeen::Absent, CommandRuns::Nothing)]
    );
    let c1 = marks_of(Held::None, b"\x1b%@", b"z\x9d133;A\x07");
    assert_eq!(
        c1,
        [(7, within(6), b'A', TokenSeen::Absent, CommandRuns::Nothing)]
    );
}

/// **The shell's `C` naming a program `exec` puts in its place takes the
/// token with it**: the `C` is `Shells` and `Replaced`, and the shell's
/// `D` after it reads `Absent`, the token gone. Another token's `exec`
/// takes nothing. Named break: keep the token at a `Replaced` `C` -- the
/// `D` after it reads `Shells`.
#[test]
fn the_shells_exec_takes_its_token() {
    let t = core::str::from_utf8(TOKEN).expect("ascii");
    let exec = format!("\x1b]133;C;baud_token={t};cmdline=exec\\ zsh\x07");
    let d = format!("\x1b]133;D;0;baud_token={t}\x07");
    let got: Vec<(TokenSeen, CommandRuns)> =
        marks_of(Held::Token, b"", &[exec.as_bytes(), d.as_bytes()].concat())
            .into_iter()
            .map(|(_, _, _, s, r)| (s, r))
            .collect();
    assert_eq!(
        got,
        [
            (TokenSeen::Shells, CommandRuns::Replaced),
            (TokenSeen::Absent, CommandRuns::Nothing)
        ]
    );
    let foreign = "\x1b]133;C;baud_token=fedcba9876543210;cmdline=exec\\ zsh\x07";
    let got: Vec<TokenSeen> = marks_of(
        Held::Token,
        b"",
        &[foreign.as_bytes(), d.as_bytes()].concat(),
    )
    .into_iter()
    .map(|(_, _, _, s, _)| s)
    .collect();
    assert_eq!(got, [TokenSeen::Other, TokenSeen::Shells]);
}

/// **A command line cut at `OSC_MAX` names its program from the whole
/// words before the cut**: `ssh` followed by 2,000 words is `Elsewhere`;
/// a program path that runs past the bound names nothing, and the token
/// in the cut option is not read. Named break: read a cut mark's command
/// line as whole (`CommandRuns::of`) -- the cut path reads as a program
/// here.
#[test]
fn a_cut_command_line_names_only_whole_words() {
    let t = core::str::from_utf8(TOKEN).expect("ascii");
    let runs = |line: String| {
        let c = format!("\x1b]133;C;baud_token={t};cmdline={line}\x07");
        marks_of(Held::Token, b"", c.as_bytes())[0]
    };
    let long = runs(format!("ssh{}", "\\ x".repeat(2000)));
    assert_eq!(
        (long.3, long.4),
        (TokenSeen::Shells, CommandRuns::Elsewhere)
    );
    let cut = runs(format!("/usr/{}/ssh", "a".repeat(OSC_MAX)));
    assert_eq!((cut.3, cut.4), (TokenSeen::Shells, CommandRuns::Nothing));
    let late = format!(
        "\x1b]133;C;cmdline=ls;pad={};baud_token={t}\x07",
        "p".repeat(OSC_MAX)
    );
    let got = marks_of(Held::Token, b"", late.as_bytes())[0];
    assert_eq!((got.3, got.4), (TokenSeen::Other, CommandRuns::Here));
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

/// **A change of the alternate screen's height keeps every row it keeps on
/// its line**: ten labelled rows, the cursor on the last, shrunk to six and
/// grown back to ten -- rows 0 to 5 keep their text on their rows, and the
/// alternate screen's top line, the record's end less the cursor's row,
/// does not move. An embedder numbers the alternate screen's lines by that
/// top (the app's `keep::Counts`), and its count follows a resize by the
/// resize's record; this is where a core that cut from the top instead
/// would show first. Break: scroll the top rows away on a shrink (four
/// line feeds at the bottom first) -- row 0 holds `r4`, and the top moves.
#[test]
fn an_alternate_height_change_keeps_each_row_it_keeps_on_its_line() {
    support::with_term(&WS, |t| {
        let mut setup = b"\x1b[?1049h".to_vec();
        for y in 0..10 {
            setup.extend(format!("\x1b[{};1Hr{y}", y + 1).into_bytes());
        }
        let _ = read(t, &setup);
        let top = |t: &mut Term<'_>| {
            let end = t.take_record().end();
            assert_eq!(end.screen, Screen::Alternate);
            end.line.minus(Rows::of_count(t.cursor().0.index()))
        };
        let was = top(t);
        let text = |t: &Term<'_>, y: usize| -> String {
            t.row(Row::from_index(y))
                .expect("a row")
                .iter()
                .filter_map(|c| c.content.as_codepoint())
                .filter_map(|c| char::from_u32(c.value()))
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        for rows in [6, 10] {
            let _ = t.resize(&WinSize {
                ws_col: WS.ws_col,
                ws_row: rows,
            });
            assert_eq!(top(t), was, "the top line at {rows} rows");
            for y in 0..6 {
                assert_eq!(text(t, y), format!("r{y}"), "row {y} at {rows} rows");
            }
        }
    });
}

/// **The mark's line is the window top a record's cursor line is counted
/// from** (`HistoryMark::line`): after each part -- lines retired, lines
/// taken back, a switch to the alternate screen and back, a re-cut by a
/// change of columns, and a scroll down past the first line of a fresh
/// terminal -- a record that ends on the primary with the primary up ends
/// on the mark's line plus the cursor's row, read at the same moment. An
/// embedder that places each part's lines against the mark it takes with
/// the record places them as the record numbers them. Named break: the
/// mark's line one past the window top -- red from the first step.
#[test]
fn the_mark_line_is_the_window_top_each_record_counts_from() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 6,
    };
    enum Step {
        Read(&'static [u8]),
        Columns(u16),
    }
    let long = b"0123456789012345678901234567890123456789\r\n";
    let steps = [
        Step::Read(b"\x1b[2T"),
        Step::Read(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\n"),
        Step::Read(long),
        Step::Read(b"\x1b[2T"),
        Step::Read(b"\x1b[?1049hx\r\ny\r\n\x1b[?1049l"),
        Step::Read(b"\x1b[?1049h\x1b[?1049lk\r\nl\r\n"),
        Step::Columns(10),
        Step::Read(b"m\r\nn\r\no\r\np\r\nq\r\nr\r\ns\r\n"),
        Step::Columns(20),
        Step::Read(b"\x1b[3T"),
    ];
    support::with_term(&ws, |t| {
        let mut checked = 0;
        let mut moved = Vec::new();
        for (k, step) in steps.iter().enumerate() {
            let was = t.history_mark();
            let rec = match step {
                Step::Read(bytes) => read(t, bytes),
                Step::Columns(c) => {
                    let _ = t.resize(&WinSize {
                        ws_col: *c,
                        ws_row: ws.ws_row,
                    });
                    t.take_record()
                }
            };
            let top = t.history_mark().line();
            moved.push(t.history_moved_since(was));
            let row = Rows::of_count(t.cursor().0.index());
            if rec.end().screen == Screen::Normal && t.screen() == Screen::Normal {
                assert_eq!(rec.end().line, top.plus(row), "step {k}");
                checked += 1;
            }
        }
        assert_eq!(checked, steps.len(), "every step ended on the primary");
        // What each step did to the window's top: the fresh terminal's
        // scroll down moves none, and the re-cuts move it by their own
        // renumbering.
        let up = |n| Moved::Retired(Rows::of_count(n));
        let back = |n| Moved::Restored(Rows::of_count(n));
        assert_eq!(
            moved,
            [
                up(0),
                up(5),
                up(2),
                back(2),
                up(0),
                up(2),
                up(1),
                up(7),
                back(1),
                back(3)
            ]
        );
    });
}

/// **Whether a read wrote the line it began on** (`StartRow`): each
/// writer, on the line the cursor stands on as the read begins -- text,
/// text after a carriage return, `REP`, `EL`, `ECH`, `ED` reaching it,
/// `ICH`, `DCH`, `DECFRA`, `DECERA`, `DECCRA` onto it -- says it wrote it;
/// so does any control function not known to write no cell run with the
/// cursor on the line, wherever it writes: `DECFRA` on the line above, a
/// switch to the alternate screen (`Paint::control_may_write`). A carriage
/// return, a line feed, a cursor move or a rendition alone, and the text
/// that follows on later lines, leave it untouched; so does `DECFRA` on
/// the line above run from there. A climb back onto it is named off the
/// path instead. Named breaks: the fact never set
/// (`Paint::record_start_rows` a no-op) -- every writer reads untouched; a
/// rectangle's row closing the window (`Paint::overwrote_unprinted`
/// without `reopened`) -- text after a rectangle run from the line above
/// reads untouched.
/// **Every scroll that copies rows over the line a read began on records a
/// move on that line's screen** (`Cold::band_moved`, `Cold::moved_in_place`):
/// a line feed at the bottom of a region below the top, the alternate
/// screen's copy of a short top region, `RI` at a region's top margin, a
/// line feed under side margins, `SU` and `SD` in a region below the top,
/// and a prompt pinned to the last row at its `133;B`. A turn of the ring
/// keeps each line's text on its own number and records none: the full
/// screen's line feed and `SU`, a top region's `SU` on the primary, `SD`
/// taking a line back from the history, and the alternate screen's `SU`
/// and `SD` while its count can move; and where it cannot -- `SD` at the
/// primary's line zero, with no history to take back -- the text moves
/// down under the numbers it had, recorded as a move of its own
/// (`record_move` in the scroll down). An embedder holds the line on any
/// move (StartRow). The one copy that records none is a shift of a band by
/// its whole height or more (`record_band`), which leaves no text in it:
/// `SU`, `SD`, `IL` and `DL` with such a count, which the core takes as
/// writing the line when the cursor is on it. The line feeds this test
/// runs shift one row in a band of at least two. Named breaks: a
/// full-width band moved with no record
/// (`band_moved` without `record_band`) -- every region's and the pin's
/// read with no move; a rectangle moved with none (`moved_in_place`
/// without `record_within`) -- the side margins' does; the scroll down at
/// line zero recording nothing -- `SD` with no history does.
#[test]
fn every_scroll_that_copies_onto_the_start_line_records_a_move() {
    let moved_onto_start = |rec: ReadRecord| {
        let start = rec.start();
        rec.moves()
            .iter()
            .any(|m| m.screen() == start.screen && m.band().holds(start.line))
    };
    let mut got: Vec<bool> = [
        (b"\x1b[5;10r\x1b[10;1Habc".as_slice(), b"\n".as_slice()),
        (b"\x1b[?1049h\x1b[1;5r\x1b[5;1Habc", b"\n"),
        (b"\x1b[5;10r\x1b[5;1Habc", b"\x1bM"),
        (b"\x1b[?69h\x1b[2;20s\x1b[5;10r\x1b[10;5Habc", b"\n"),
        (b"\x1b[5;10r\x1b[7;1Habc", b"\x1b[S"),
        (b"\x1b[5;10r\x1b[7;1Habc", b"\x1b[T"),
        (b"\x1b[3;1Habc", b"\x1b[T"),
    ]
    .into_iter()
    .map(|(before, bytes)| moved_onto_start(after(&WS, before, bytes)))
    .collect();
    got.push(moved_onto_start(support::with_term(&WS, |t| {
        t.set_prompt_at_bottom(baud_domain::PromptAtBottom::On);
        let _ = read(t, b"out\r\n");
        read(t, b"\x1b]133;A\x07$ \x1b]133;B\x07")
    })));
    // The ring's turns: no line's text moves off its number.
    let history = [b"x\r\n".repeat(40), b"\x1b[3;1Habc".to_vec()].concat();
    for (before, bytes) in [
        (b"\x1b[24;1Habc".as_slice(), b"\n".as_slice()),
        (b"\x1b[7;1Habc", b"\x1b[S"),
        (b"\x1b[1;10r\x1b[7;1Habc", b"\x1b[S"),
        (&history, b"\x1b[T"),
        (b"\x1b[?1049h\x1b[3;1Habc", b"\x1b[S"),
        (b"\x1b[?1049h\x1b[3;1Habc", b"\x1b[T"),
    ] {
        got.push(moved_onto_start(after(&WS, before, bytes)));
    }
    // The region's line feed, the alternate's, `RI`'s, the side margins',
    // the region's `SU` and `SD`, `SD` with no history, the pin's; then the
    // ring's six.
    assert_eq!(
        got,
        [
            true, true, true, true, true, true, true, true, false, false, false, false, false,
            false
        ]
    );
}

/// **A control function that may write a cell, run with the cursor on the
/// line a read began on, says it wrote it, though no list of writers names
/// it** (`Paint::control_may_write`): `IL` and `DL` on the bottom margin,
/// which record no row and no move, and `DECCARA` and `DECRARA` on the
/// line alone, which tell no row. Named break: the function placed among
/// those that write no cell (`write::writes_no_cell`) -- it reads
/// untouched.
#[test]
fn a_control_function_not_known_to_write_nothing_writes_the_start_line() {
    for (before, bytes) in [
        (b"\x1b[24;1Habc".as_slice(), b"\x1b[L".as_slice()),
        (b"\x1b[24;1Habc", b"\x1b[M"),
        (b"\x1b[1;1Habc", b"\x1b[1;1;1;3;7$r"),
        (b"\x1b[1;1Habc", b"\x1b[1;1;1;3;7$t"),
    ] {
        assert_eq!(
            after(&WS, before, bytes).start_row(),
            StartRow::Wrote,
            "{bytes:?}"
        );
    }
}

/// **Text on the status display neither decides nor closes the window on
/// the line a read began on**: the status line taken, text written there,
/// the main display back and text on the line -- it wrote the line. The
/// control: the same with no text on the line -- untouched. Named break:
/// text on the status display closing the window (`overwrote_hooked`
/// without its `on_status` guard) -- the line's text reads untouched.
#[test]
fn text_on_the_status_line_leaves_the_start_lines_window_open() {
    let before = b"\x1b[2$~abc";
    assert_eq!(
        after(&WS, before, b"\x1b[1$}s\x1b[0$}\rx").start_row(),
        StartRow::Wrote
    );
    assert_eq!(
        after(&WS, before, b"\x1b[1$}s\x1b[0$}").start_row(),
        StartRow::Untouched,
        "the control"
    );
}

/// **A reset writes the line a read began on, wherever the cursor stands**:
/// `RIS` from the alternate screen after it scrolled, with the cursor on
/// the line and a row below it. Named break: the reset leaving the fact as
/// it was (`hard_reset` without `start_wrote`) -- the row below reads
/// untouched.
#[test]
fn a_reset_writes_the_start_line() {
    let before = [b"\x1b[?1049h".as_slice(), &[b'\n'; 30], b"abc"].concat();
    for bytes in [b"\x1bc".as_slice(), b"\n\x1bc"] {
        assert_eq!(
            after(&WS, &before, bytes).start_row(),
            StartRow::Wrote,
            "{bytes:?}"
        );
    }
}

/// **A read's record says whether it wrote the line it began on**
/// (`ReadRecord::start_row`), the cursor on a prompt holding `/etc/hosts`:
/// text there, an erase or edit reaching it, a rectangle over it and a
/// switch to the alternate screen each write it; a return, a line feed, a
/// move, a rendition and output on the lines below leave it untouched, the
/// control. A rectangle written on another row is not the read's text, so
/// text on the line after it still writes it; and a climb back to the line
/// after leaving it is recorded off the path, the line itself untouched.
/// Break: a rectangle no longer leaves the window as it found it
/// (`Recording::reopened` doing nothing) -- the text after it reads
/// untouched.
#[test]
fn a_read_says_whether_it_wrote_the_line_it_began_on() {
    let prompt = b"$ /etc/hosts\r\nx\r\n\x1b[A";
    let start = |bytes: &[u8]| after(&WS, prompt, bytes).start_row();
    for wrote in [
        b"z".as_slice(),
        b"\rz",
        b"z\x1b[3b",
        b"\x1b[K",
        b"\x1b[2X",
        b"\x1b[B\x1b[1J",
        b"\x1b[@",
        b"\x1b[P",
        b"\x1b[46;2;1;2;5$x",
        b"\x1b[2;1;2;5$z",
        b"\x1b[1;1;1;5;1;2;1;1$v",
        b"\x1b[46;1;1;1;5$x",
        b"\x1b[?1049hz\x1b[K",
    ] {
        assert_eq!(start(wrote), StartRow::Wrote, "{wrote:?}");
    }
    for untouched in [
        b"\r".as_slice(),
        b"\r\n",
        b"\n",
        b"\x1b[5;5H",
        b"\x1b[1;31m",
        b"\r\nls output\r\n",
        b"\x1b[A\x1b[46;1;1;1;5$x\x1b[B",
    ] {
        assert_eq!(start(untouched), StartRow::Untouched, "{untouched:?}");
    }
    // A rectangle written on another row is not the read's text: the
    // line is still open to it.
    for rect_then_text in [
        b"\x1b[A\x1b[46;1;1;1;5$x\x1b[B\rz".as_slice(),
        b"\x1b[A\x1b[2;1;2;5;1;1;1;1$v\x1b[B\rz",
    ] {
        assert_eq!(start(rect_then_text), StartRow::Wrote, "{rect_then_text:?}");
    }
    let climbed = after(&WS, prompt, b"\r\nout\x1b[Az");
    assert_eq!(climbed.start_row(), StartRow::Untouched);
    assert_eq!(climbed.off_path(Screen::Normal), rows(1, 2));
}

/// The lines of `screen` `rec` says it erased whole, or none for a record
/// that held more than it has room for.
fn erased(rec: &ReadRecord, screen: Screen) -> Option<Vec<Span>> {
    rec.erased(screen).map(Iterator::collect)
}

/// The primary's rows `a` to `b`, erased whole.
fn whole(a: usize, b: usize) -> Option<Vec<Span>> {
    Some(vec![Span::of(line(a), line(b))])
}

/// Nothing erased whole, with room.
fn none_whole() -> Option<Vec<Span>> {
    Some(Vec::new())
}

/// **A line erased whole is recorded, exactly** (`ReadRecord::erased`):
/// `ED 2` every row; `EL 2` its row; `EL 0` from the first column its row
/// and from column 6 none; `ED 0` from column 4 the rows below and from
/// the first column the cursor's row with them, one span; `ED 1` and `EL 1`
/// through the last column the cursor's row, and `ED 1` short of it only
/// the rows above; a run of `EL 2` down the screen one span; `RIS` and
/// `DECCOLM`'s clear every row. Named break: no erasure recorded
/// (`Paint::record_erased` doing nothing) -- every one reads none.
#[test]
fn a_whole_line_erasure_is_recorded_exactly() {
    let at = |before: &[u8], bytes: &[u8]| erased(&after(&WS, before, bytes), Screen::Normal);
    let text = b"\x1b[6;1Hx".as_slice();
    assert_eq!(at(text, b"\x1b[2J"), whole(0, 9), "ED 2");
    assert_eq!(at(b"\x1b[4;8H", b"\x1b[2K"), whole(3, 3), "EL 2");
    assert_eq!(at(b"\x1b[4;1H", b"\x1b[K"), whole(3, 3), "EL 0, column 1");
    assert_eq!(at(b"\x1b[4;6H", b"\x1b[K"), none_whole(), "EL 0, column 6");
    assert_eq!(at(b"\x1b[4;4H", b"\x1b[J"), whole(4, 9), "ED 0, column 4");
    assert_eq!(at(b"\x1b[4;1H", b"\x1b[J"), whole(3, 9), "ED 0, column 1");
    assert_eq!(
        at(b"\x1b[4;20H", b"\x1b[1J"),
        whole(0, 3),
        "ED 1, last column"
    );
    assert_eq!(at(b"\x1b[4;6H", b"\x1b[1J"), whole(0, 2), "ED 1, column 6");
    assert_eq!(
        at(b"\x1b[4;20H", b"\x1b[1K"),
        whole(3, 3),
        "EL 1, last column"
    );
    assert_eq!(at(b"\x1b[4;6H", b"\x1b[1K"), none_whole(), "EL 1, column 6");
    let run: Vec<u8> = (1..=10)
        .flat_map(|y| format!("\x1b[{y};1H\x1b[2K").into_bytes())
        .collect();
    assert_eq!(at(b"", &run), whole(0, 9), "a run of EL 2");
    assert_eq!(at(text, b"\x1bc"), whole(0, 9), "RIS");
    assert_eq!(at(b"\x1b[?40h", b"\x1b[?3h"), whole(0, 9), "DECCOLM");
}

/// **An erase that leaves text on a line is not recorded**: `ECH` over the
/// whole row, `DECSEL` and `DECSED`, `ED 2` and `EL 2` sparing a guarded
/// area under ERM's PROTECT (`[[source: ecma48]]` 7.2.4), `EL 2` on a
/// double-width line -- its half, the text past it kept for a single-width
/// rendition to show again -- and `EL 2` on the status display, which is
/// no line. The controls: ERM's ALL erases the guarded area, and a
/// single-width line's `EL 2`, each recorded. Named break: every erase
/// recorded whole -- each reads a span.
#[test]
fn an_erase_that_leaves_text_on_the_line_is_not_recorded() {
    let at = |before: &[u8], bytes: &[u8]| erased(&after(&WS, before, bytes), Screen::Normal);
    let row = b"\x1b[4;1Habcdefghijklmnopqrs".as_slice();
    assert_eq!(at(row, b"\x1b[4;1H\x1b[20X"), none_whole(), "ECH");
    assert_eq!(at(row, b"\x1b[?2K"), none_whole(), "DECSEL");
    assert_eq!(at(row, b"\x1b[?2J"), none_whole(), "DECSED");
    let guarded = b"\x1b[4;1H\x1bVab\x1bW".as_slice();
    assert_eq!(at(guarded, b"\x1b[2J"), none_whole(), "ED 2, PROTECT");
    assert_eq!(at(guarded, b"\x1b[2K"), none_whole(), "EL 2, PROTECT");
    assert_eq!(
        at(guarded, b"\x1b[6h\x1b[2J"),
        whole(0, 9),
        "the control: ALL"
    );
    assert_eq!(at(b"\x1b[4;1H\x1b#6ab", b"\x1b[2K"), none_whole(), "DECDWL");
    assert_eq!(at(b"\x1b[4;1Hab", b"\x1b[2K"), whole(3, 3), "the control");
    let status = b"\x1b[2$~\x1b[1$}abc".as_slice();
    assert_eq!(at(status, b"\x1b[2K"), none_whole(), "the status display");
}

/// **An erasure a later move of the read reaches is dropped** (E4): the
/// line may hold text moved onto it. `IL` and `DL` above it, `RI` at a
/// region's top margin, `SU` in a region holding it and `SD` with no
/// history each drop `EL 2`'s row 4; the controls keep it -- `IL` below
/// it, `RI` off the margin, `SU` in a region below it, a full-screen `SU`
/// whose lines keep their numbers, and a move before the erase. Named
/// break: keep every erasure across a move -- each reads row 4.
#[test]
fn an_erasure_a_later_move_reaches_is_dropped() {
    let at = |before: &[u8], bytes: &[u8]| erased(&after(&WS, before, bytes), Screen::Normal);
    let row = b"\x1b[4;1Habc".as_slice();
    let region = b"\x1b[2;8r\x1b[4;1Habc".as_slice();
    let below = b"\x1b[6;10r\x1b[4;1Habc".as_slice();
    for (before, bytes, what) in [
        (row, b"\x1b[2K\x1b[2;1H\x1b[L".as_slice(), "IL above"),
        (row, b"\x1b[2K\x1b[2;1H\x1b[M", "DL above"),
        (region, b"\x1b[2K\x1b[2;1H\x1bM", "RI at the top margin"),
        (region, b"\x1b[2K\x1b[S", "SU in the region"),
        (row, b"\x1b[2K\x1b[T", "SD with no history"),
    ] {
        assert_eq!(at(before, bytes), none_whole(), "{what}");
    }
    for (before, bytes, what) in [
        (row, b"\x1b[2K\x1b[6;1H\x1b[L".as_slice(), "IL below"),
        (region, b"\x1b[2K\x1b[6;1H\x1bM", "RI off the margin"),
        (below, b"\x1b[4;1H\x1b[2K\x1b[S", "SU in a region below"),
        (row, b"\x1b[2K\x1b[S", "a full-screen SU"),
        (
            row,
            b"\x1b[2;1H\x1b[L\x1b[4;1H\x1b[2K",
            "IL before the erase",
        ),
    ] {
        assert_eq!(at(before, bytes), whole(3, 3), "{what}");
    }
}

/// **More erasures than the record holds vouch for none** (E1): four
/// apart are each recorded, and a fifth overflows the list, which then
/// names no line erased on either screen, so an embedder forgets nothing.
/// Named break: drop the fifth silently -- the four read as all.
#[test]
fn more_erasures_than_the_record_holds_vouch_for_none() {
    let rows_apart = |n: usize| -> Vec<u8> {
        (0..n)
            .flat_map(|k| format!("\x1b[{};1H\x1b[2K", 1 + 2 * k).into_bytes())
            .collect()
    };
    let four = after(&WS, b"", &rows_apart(4));
    let spans: Vec<Span> = (0..4).map(|k| Span::of(line(2 * k), line(2 * k))).collect();
    assert_eq!(erased(&four, Screen::Normal), Some(spans), "the control");
    let five = after(&WS, b"", &rows_apart(5));
    assert_eq!(erased(&five, Screen::Normal), None);
    assert_eq!(erased(&five, Screen::Alternate), None);
}

/// **`EL 2` under side margins erases the whole line, and is recorded**
/// (E3): EL clears to `end_of_line(line)` (`[[source: dec-std-070]]` EL,
/// p. 5-155), the line's width and not the right margin, and
/// `Paint::line_span` spans `line_cols`, every column of a single-width
/// line, whatever `DECSLRM` set. A row of text, margins 5 to 15, `EL 2`
/// from column 8: every cell of the row is blank, those outside the
/// margins included, and the row is erased whole. Named break: `EL`
/// bounded by the side margins -- the cells outside them keep their text.
#[test]
fn el_2_under_side_margins_erases_and_records_the_whole_line() {
    let before = b"\x1b[4;1Habcdefghijklmnopqrs\x1b[?69h\x1b[5;15s\x1b[4;8H";
    let (rec, text) = support::with_term(&WS, |t| {
        let _ = read(t, before);
        let rec = read(t, b"\x1b[2K");
        let row = t.row(Row::from_index(3)).expect("row 4");
        let text: String = row
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|c| char::from_u32(c.value()))
                    .unwrap_or(' ')
            })
            .collect();
        (rec, text)
    });
    assert_eq!(text, " ".repeat(20), "every cell blank");
    assert_eq!(erased(&rec, Screen::Normal), whole(3, 3));
}

/// **An erasure of the alternate screen is its own, and the screen's clear
/// drops it**: `ED 2` there names the alternate screen's ten lines and
/// none of the primary's; a read that enters with `?1049 h`, whose clear
/// starts the screen's count again, names none on either. Named break:
/// keep the alternate screen's erasures across its clear
/// (`Recording::cleared_alternate`) -- the entry reads the clear's rows.
#[test]
fn an_alternate_screen_erasure_is_its_own_and_a_clear_drops_it() {
    let ed = after(&WS, b"\x1b[?1049h", b"\x1b[2J");
    let top = ed.start().line;
    assert_eq!(ed.start().screen, Screen::Alternate);
    assert_eq!(
        erased(&ed, Screen::Alternate),
        Some(vec![Span::of(top, top.plus(Rows::of_count(9)))])
    );
    assert_eq!(erased(&ed, Screen::Normal), none_whole());
    let entry = after(&WS, b"abc", b"\x1b[?1049h");
    assert_eq!(erased(&entry, Screen::Alternate), none_whole());
    assert_eq!(erased(&entry, Screen::Normal), none_whole());
}

/// **A command line is read once a `C` mark, and no byte of text or of
/// another sequence reads one**: 4,096 bytes with an `SGR` every 17 read
/// none; three `C`s and a `D` among them read three. Named break: bump the
/// count at every `OSC 133` -- the `D` reads a fourth.
#[test]
fn a_command_line_is_read_once_a_command_mark() {
    let sgr: Vec<u8> = b"\x1b[1mabcdefghijkl"
        .iter()
        .copied()
        .cycle()
        .take(4096)
        .collect();
    let count = |bytes: &[u8]| {
        support::with_term(&WS, |t| {
            let before = t.command_lines_read().get();
            let _ = read(t, bytes);
            t.command_lines_read().get() - before
        })
    };
    assert_eq!(count(&sgr), 0, "text and SGRs");
    let c = b"\x1b]133;C;cmdline=ls\x07";
    let marks = [&sgr[..100], c, c, b"\x1b]133;D;0\x07", c].concat();
    assert_eq!(count(&marks), 3, "three Cs");
}
