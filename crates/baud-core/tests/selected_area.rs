//! **SSA and ESA, the selected area, and the three modes that choose what a
//! print takes of it: SATM, MATM and TTM** (`[[source: ecma48]]`), the
//! standard's meaning of `ESC F` alone: HP's, the cursor to the lower left
//! corner, is off in xterm by default (`hpLowerleftBugCompat`) and is not
//! built.
//!
//! * 6.5.1: "The character position in the presentation component which is
//!   the active presentation position after the receipt of SSA is the
//!   first character position of the selected area ... The character
//!   position ... which is the active presentation position before the
//!   receipt of ESA is the last character position of the selected area."
//! * 7.2.12 MATM, 7.2.14 SATM, 7.2.18 TTM, reset SINGLE, SELECT and CURSOR
//!   (Table 6), and 7.3.1 a to f, which read the three together: "a) If the
//!   TTM is set to CURSOR, the SATM to SELECT, and the MATM to SINGLE, then
//!   the contents of the active selected area, up to but excluding the
//!   active presentation position, are eligible" ... "f) If the TTM and the
//!   SATM are both set to ALL, then the complete contents of the buffer are
//!   eligible", and g and h for GATM. "If the active presentation position
//!   is not within a selected area, the format of the data stream in the
//!   first and fourth case above is not defined by this Standard."
//!
//! A print is MC's transfer (6.6, 8.3.82): print page, screen, all pages,
//! print line and autoprint. An ineligible position prints as a space, as
//! GATM's guarded one does, so what is eligible keeps its columns; with
//! areas defined and the active position in none, cases a and d print as
//! no area does (a reading the text leaves open); each end is kept as
//! SPA's start is, by line, so an area moves with its text, and IL and DL
//! shift it, as 8.3.67 and 8.3.32 require. Nothing changes on the screen.

mod support;

use baud_core::*;
use baud_domain::{PrintBudget, PrinterPort, PromptAtBottom};

const WS: WinSize = WinSize {
    ws_col: 10,
    ws_row: 4,
};

/// Row 0 `abcdefgh`, its area from column 3 (`c`) to column 7, the
/// position before ESA; row 1 `xyz`, its area from column 1 to 3.
const TWO: &[u8] = b"ab\x1bFcdef\x1bGgh\r\n\x1bFxy\x1bGz";

fn owned() -> Owned {
    Owned::new(&WS).with_printer(PrintBudget::of_bytes(4096))
}

/// The print page `bytes` then `CSI i` makes, read `read` bytes at a time.
fn printed_at(bytes: &[u8], read: usize) -> (String, Vec<PackedCell>, (Row, Col), usize) {
    let pieces: Vec<&[u8]> = bytes.chunks(read).collect();
    printed_in(&pieces)
}

/// The same after `pieces`, each a read.
fn printed_in(pieces: &[&[u8]]) -> (String, Vec<PackedCell>, (Row, Col), usize) {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer(PrinterPort::Ready);
    for piece in pieces {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let screen = support::screen(&t);
    let cursor = t.cursor();
    support::feed(&mut t, b"\x1b[i");
    let jobs: Vec<String> = t
        .print_jobs()
        .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
        .collect();
    assert_eq!(jobs.len(), 1, "one print");
    (jobs[0].clone(), screen, cursor, t.unsupported)
}

/// `want` printed after `bytes` at every read size and every split into
/// two reads.
fn prints_at_every_read(bytes: &[u8], want: &str) {
    for read in 1..=bytes.len() {
        assert_eq!(
            printed_at(bytes, read).0,
            want,
            "{read}-byte reads of {bytes:?}"
        );
    }
    for at in 1..bytes.len() {
        let got = printed_in(&[&bytes[..at], &bytes[at..]]).0;
        assert_eq!(got, want, "{bytes:?} split at {at}");
    }
}

/// MATM MULTIPLE and TTM ALL, case e: every area whole, so a print shows
/// where the areas are and nothing else.
const EVERY_AREA: &[u8] = b"\x1b[15h\x1b[16h";

/// A print page of four rows, each `rows[i]` after its line attribute.
fn page(rows: [&str; 4]) -> String {
    let body: String = rows.iter().map(|r| format!("\x1b#5{r}\r\n")).collect();
    format!("\x1b[0m{body}\x1b[0m")
}

/// **With no area defined a print is as it was**: under the reset modes,
/// case a, the active position is in no selected area and the format is
/// not defined, so the print takes the screen as before -- the control
/// every other case departs from.
#[test]
fn with_no_area_a_print_is_as_built() {
    prints_at_every_read(b"abcdefgh\r\nxyz", &page(["abcdefgh", "xyz", "", ""]));
}

/// **Case a, the reset modes: the active area up to the cursor.** The
/// cursor on column 5, inside row 0's area, takes `cd`, the area's
/// positions before it; the rest print as spaces, and trailing ones are
/// left off.
#[test]
fn case_a_takes_the_active_area_up_to_the_cursor() {
    let p = [TWO, b"\x1b[1;5H"].concat();
    prints_at_every_read(&p, &page(["  cd", "", "", ""]));
}

/// **Case d, TTM ALL: the whole active area**, `cdefg` from column 3 to
/// the position before ESA, `g` written there after.
#[test]
fn case_d_takes_the_whole_active_area() {
    let p = [TWO, b"\x1b[16h\x1b[1;5H"].concat();
    prints_at_every_read(&p, &page(["  cdefg", "", "", ""]));
}

/// **Cases a and d with the cursor in no area print as no area does**:
/// the text leaves the format undefined, and baud prints as it did before
/// any area (a reading the text leaves open).
#[test]
fn cases_a_and_d_with_the_cursor_outside_every_area_print_as_built() {
    let all = page(["abcdefgh", "xyz", "", ""]);
    prints_at_every_read(&[TWO, b"\x1b[3;1H"].concat(), &all);
    prints_at_every_read(&[TWO, b"\x1b[16h\x1b[3;1H"].concat(), &all);
}

/// **Case b, MATM MULTIPLE: every area up to the cursor**: row 0's whole,
/// above the cursor, and row 1's `x`, before it.
#[test]
fn case_b_takes_every_area_up_to_the_cursor() {
    let p = [TWO, b"\x1b[15h\x1b[2;2H"].concat();
    prints_at_every_read(&p, &page(["  cdefg", "x", "", ""]));
}

/// **Case e, MATM MULTIPLE and TTM ALL: every area whole**, row 1's to
/// the position before its ESA, where `z` was written after.
#[test]
fn case_e_takes_every_area_whole() {
    let p = [TWO, b"\x1b[15h\x1b[16h\x1b[1;1H"].concat();
    prints_at_every_read(&p, &page(["  cdefg", "xyz", "", ""]));
}

/// **Case b with no area takes nothing**: "the contents of any selected
/// area", and there is none, so each row prints empty.
#[test]
fn case_b_with_no_area_takes_nothing() {
    prints_at_every_read(b"abcdefgh\r\nxyz\x1b[15h", &page(["", "", "", ""]));
}

/// **Case c, SATM ALL: the buffer up to the cursor**, whatever the areas:
/// row 0 whole and row 1's `x`. DEC STD 070-7's print page takes the rows
/// whole; ECMA-48's mode decides what of them is eligible, and under SATM
/// SELECT, the reset state, no print changes unless an area is defined.
#[test]
fn case_c_takes_the_buffer_up_to_the_cursor() {
    let p = [TWO, b"\x1b[17h\x1b[2;2H"].concat();
    prints_at_every_read(&p, &page(["abcdefgh", "x", "", ""]));
}

/// **Case f, SATM and TTM ALL: everything.**
#[test]
fn case_f_takes_everything() {
    let p = [TWO, b"\x1b[17h\x1b[16h\x1b[2;2H"].concat();
    prints_at_every_read(&p, &page(["abcdefgh", "xyz", "", ""]));
}

/// **GATM still applies inside an area** (7.3.1 g and h): `cd` guarded
/// inside an area from column 1 prints as spaces under GUARD, and whole
/// under ALL; `qq` on row 1, outside the area, under neither.
#[test]
fn a_guarded_area_inside_a_selected_area_is_left_out_under_gatm_guard() {
    let p = b"\x1bFab\x1bVcd\x1bW\x1bG\r\nqq\x1b[16h\x1b[1;2H";
    prints_at_every_read(p, &page(["ab", "", "", ""]));
    prints_at_every_read(&[&p[..], b"\x1b[1h"].concat(), &page(["abcd", "", "", ""]));
}

/// **Print line and print screen take the same eligible contents**: the
/// cursor's line under case a, and the whole screen.
#[test]
fn print_line_and_print_screen_take_the_selected_contents() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer(PrinterPort::Ready);
    support::feed(&mut t, &[TWO, b"\x1b[1;5H\x1b[?1i\x1b[?10i"].concat());
    let jobs: Vec<String> = t
        .print_jobs()
        .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
        .collect();
    assert_eq!(
        jobs,
        [
            "\x1b[0m\x1b#5  cd\r\n\x1b[0m".to_string(),
            page(["  cd", "", "", ""]),
        ]
    );
}

/// **On a right-to-left line the order is the line's path**: the
/// positions along a line are numbered along its character path
/// (`crate::component`), so "the character positions in a line of a
/// selected area are ordered according to the character path of this
/// line" (6.5.1) and TTM CURSOR's "preceding" read the same columns on a
/// line SCP turned, with BDSM set and reset: `cd`, before the cursor.
#[test]
fn on_a_right_to_left_line_the_order_is_the_lines_path() {
    for bdsm in [&b""[..], b"\x1b[8l"] {
        let p = [bdsm, b"\x1b[2 k", TWO, b"\x1b[1;5H"].concat();
        prints_at_every_read(&p, &page(["  cd", "", "", ""]));
    }
}

/// **An area moves with its text**: row 1's area scrolled to row 0 is
/// printed there, and `q`, outside it, is not, on either screen: the
/// alternate screen's numbering moves while an area is kept there.
#[test]
fn an_area_moves_with_its_text_as_the_screen_scrolls() {
    for screen in [&b""[..], b"\x1b[?1049h"] {
        let p = [screen, b"\x1b[2;1H\x1bFxy\x1bG\x1b[4;1Hq\n\x1b[15h\x1b[16h"].concat();
        prints_at_every_read(&p, &page(["xy", "", "", ""]));
    }
}

/// **An area scrolled wholly above the screen selects nothing**: its ends
/// both above, it is no area of the first position. Under case e nothing
/// prints; under the reset modes with the cursor at home it is in no area,
/// so the print is as built.
#[test]
fn an_area_scrolled_wholly_above_the_screen_selects_nothing() {
    let gone = b"\x1bFab\x1bG\x1b[4;1H\n\n\n\n\x1b[1;1HQRS";
    prints_at_every_read(&[gone, EVERY_AREA].concat(), &page(["", "", "", ""]));
    let home = [&gone[..], b"\x1b[H"].concat();
    prints_at_every_read(&home, &page(["QRS", "", "", ""]));
}

/// **IL and DL shift an area's ends in the shifted part**: "Any
/// occurrences of the start or end of a selected area ... in the shifted
/// part, are also shifted" (`[[source: ecma48]]` 8.3.67, 8.3.32). IL on
/// row 0 takes row 1's area to row 2 with `xy`, on either screen; DL of
/// row 0 takes row 2's area to row 1, and `QQ`, written on row 2 after,
/// is outside it.
#[test]
fn il_and_dl_shift_an_area_with_its_lines() {
    for screen in [&b""[..], b"\x1b[?1049h"] {
        let il = [screen, b"ab\r\n\x1bFxy\x1bG\x1b[1;1H\x1b[L", EVERY_AREA].concat();
        prints_at_every_read(&il, &page(["", "", "xy", ""]));
        let dl = [
            screen,
            b"ab\r\n\r\n\x1bFxy\x1bG\x1b[1;1H\x1b[M\x1b[3;1HQQ",
            EVERY_AREA,
        ]
        .concat();
        prints_at_every_read(&dl, &page(["", "xy", "", ""]));
    }
}

/// **IL inside an area moves only the ends after it**: the area from row
/// 0 to row 1's third position grows to row 2's, `Q` past it left out.
#[test]
fn il_inside_an_area_moves_only_its_last_end() {
    let p = [
        &b"\x1bFab\r\nxy\x1bG\x1b[2;1H\x1b[L\x1b[3;6HQ"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&p, &page(["ab", "", "xy", ""]));
}

/// **An end on a line removed takes its area**, which no text decides,
/// as SPA's open start is forgotten: DL of the area's line, and IL
/// pushing it out past the bottom margin. `QQ` written there after is in
/// no area.
#[test]
fn an_end_on_a_line_removed_takes_its_area() {
    let dl = [
        &b"\x1b[2;1H\x1bFxy\x1bG\x1b[2;1H\x1b[M\x1b[2;1HQQ"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&dl, &page(["", "", "", ""]));
    let il = [
        &b"\x1b[4;1H\x1bFxy\x1bG\x1b[1;1H\x1b[L\x1b[4;1HQQ"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&il, &page(["", "", "", ""]));
}

/// **An open SSA's start moves with IL too**, "the start ... of a
/// selected area": opened on row 1, shifted to row 2, ended there.
#[test]
fn an_open_start_moves_with_il_and_esa_ends_it_there() {
    let p = [
        &b"ab\r\n\x1bFxy\x1b[1;1H\x1b[L\x1b[3;3H\x1bG"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&p, &page(["", "", "xy", ""]));
}

/// **Under side margins only the ends between them move**: IL with
/// margins 1 to 5 takes `xy`'s area down with it, and `pq`'s, right of
/// the margin, stays with `pq`; `Q`, in neither, is left out.
#[test]
fn under_side_margins_only_the_ends_between_them_move() {
    let p = [
        &b"\x1b[2;1H\x1bFxy\x1bG\x1b[2;7H\x1bFpq\x1bG\x1b[4;9HQ\x1b[?69h\x1b[1;5s\x1b[1;1H\x1b[L"[..],
        b"\x1b[?69l",
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&p, &page(["", "      pq", "xy", ""]));
}

/// **A region's scroll moves an area with its text**, as it moves SPA's
/// start, which no text decides: a region from row 1 to row 3 scrolled
/// by a line feed takes row 2's area to row 1; under a top-anchored
/// region, rows 0 to 2, the area on row 3 below it stays with its text
/// where the window's move would walk it up to `CCC`'s row. A reverse
/// index at the region's top takes row 1's area down to row 2.
#[test]
fn a_regions_scroll_moves_an_area_with_its_text() {
    let copy = [
        &b"\x1b[3;1H\x1bFxy\x1bG\x1b[2;4r\x1b[4;1H\n\x1b[3;1HQQ\x1b[r"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&copy, &page(["", "xy", "", ""]));
    let down = [
        &b"\x1b[2;1H\x1bFxy\x1bG\x1b[2;4r\x1b[2;1H\x1bM\x1b[2;1HQQ\x1b[r"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&down, &page(["", "", "xy", ""]));
    let under = [
        &b"\x1b[4;1H\x1bFxy\x1bG\x1b[1;3r\x1b[3;1H\n\x1b[3;1HCCC\x1b[r"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&under, &page(["", "", "", "xy"]));
}

/// **A prompt walking down to the last row takes its area with it**: the
/// rows of a shell's block move as a region's do (`PromptAtBottom::On`),
/// so the area around `$ ` lands on row 3, and `QQ`, written on row 0
/// after, is outside it.
#[test]
fn a_prompt_walking_down_takes_its_area_with_it() {
    let bytes = b"\x1b]133;A\x07\x1bF$ \x1bG\x1b]133;B\x07\x1b[1;1HQQ\x1b[15h\x1b[16h";
    let printed = |pieces: &[&[u8]]| {
        let mut s = owned();
        let mut t = Term::new(s.storage(), &WS);
        t.set_printer(PrinterPort::Ready);
        t.set_prompt_at_bottom(PromptAtBottom::On);
        for piece in pieces {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        support::feed(&mut t, b"\x1b[i");
        let job = t
            .print_jobs()
            .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
            .next();
        job
    };
    let want = Some(page(["", "", "", "$"]));
    for read in 1..=bytes.len() {
        let pieces: Vec<&[u8]> = bytes.chunks(read).collect();
        assert_eq!(printed(&pieces), want, "{read}-byte reads");
    }
    for at in 1..bytes.len() {
        assert_eq!(
            printed(&[&bytes[..at], &bytes[at..]]),
            want,
            "split at {at}"
        );
    }
}

/// **A reverse index at the top moves an area down with its text**: the
/// screen's numbering cannot count below its first line, so the ends move
/// instead, as SPA's start does, and `QQQ` on row 1 is outside it, on
/// either screen.
#[test]
fn a_reverse_index_at_the_top_moves_an_area_down() {
    for screen in [&b""[..], b"\x1b[?1049h"] {
        let p = [
            screen,
            b"\x1b[2;1H\x1bFxy\x1bG\x1b[1;1H\x1bM\x1b[2;1HQQQ",
            EVERY_AREA,
        ]
        .concat();
        prints_at_every_read(&p, &page(["", "", "xy", ""]));
    }
}

/// **Leaving the alternate screen forgets its areas**, an open start too:
/// the next entry numbers its lines from the first again, so a kept area
/// would name the next program's `QRS`. The control: without leaving, the
/// area is `QRS`'s.
#[test]
fn leaving_the_alternate_screen_forgets_its_areas() {
    let kept = b"\x1b[?1049h\x1bFab\x1bG\x1b[?1049l\x1b[?1049h\x1b[1;1HQRS";
    prints_at_every_read(&[kept, EVERY_AREA].concat(), &page(["", "", "", ""]));
    let open = b"\x1b[?1049h\x1bF\x1b[?1049l\x1b[?1049h\x1b[1;1HQRS\x1bG";
    prints_at_every_read(&[open, EVERY_AREA].concat(), &page(["", "", "", ""]));
    let stayed = b"\x1b[?1049h\x1bFab\x1bG\x1b[1;1HQRS";
    prints_at_every_read(&[stayed, EVERY_AREA].concat(), &page(["QRS", "", "", ""]));
}

/// **A change of geometry forgets the areas**, as it forgets an open SPA:
/// a widen joins the wrapped row and renumbers the lines under it, so
/// `xy`'s area would name `QQ`'s line. The same geometry keeps it.
#[test]
fn a_change_of_geometry_forgets_the_areas() {
    const WIDE: WinSize = WinSize {
        ws_col: 20,
        ws_row: 4,
    };
    for (to, want) in [
        (WIDE, page(["", "", "", ""])),
        (WS, page(["", "", "xy", ""])),
    ] {
        let mut s = Owned::new(&WIDE).with_printer(PrintBudget::of_bytes(4096));
        let mut t = Term::new(s.storage(), &WS);
        t.set_printer(PrinterPort::Ready);
        support::feed(&mut t, b"0123456789AB\r\n\x1bFxy\x1bG\r\nQQ");
        support::feed(&mut t, EVERY_AREA);
        let _ = t.resize(&to);
        support::feed(&mut t, b"\x1b[i");
        let job = t
            .print_jobs()
            .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
            .next();
        assert_eq!(job.as_deref(), Some(want.as_str()), "to {}", to.ws_col);
    }
}

/// **SSA and ESA written to the status line select nothing**, a reading
/// the text leaves open: it is a display of its own, not the page a print
/// takes, so `QRS` on the main display's first row is in no area. The
/// main display is the alternate screen, whose numbering the line's
/// writes share.
#[test]
fn ssa_and_esa_on_the_status_line_select_nothing() {
    let p = [
        &b"\x1b[?1049h\x1b[2$~\x1b[1$}\x1bFab\x1bG\x1b[0$}\x1b[1;1HQRS"[..],
        EVERY_AREA,
    ]
    .concat();
    prints_at_every_read(&p, &page(["", "", "", ""]));
}

/// **Nothing changes on the screen, and neither is counted**: the cells
/// and the cursor are those of the same text without SSA and ESA, at every
/// read size and every two-read split, under `ESC F` and `ESC G` and
/// under ISO 8859-1's `86` and `87`, and the print is the same at each.
#[test]
fn ssa_and_esa_change_nothing_on_screen_at_every_read_size() {
    for (with, without) in [
        (
            [TWO, b"\x1b[1;5H"].concat(),
            b"abcdefgh\r\nxyz\x1b[1;5H".to_vec(),
        ),
        (
            b"\x1b%@ab\x86cdef\x87gh\r\n\x86xy\x87z\x1b[1;5H".to_vec(),
            b"\x1b%@abcdefgh\r\nxyz\x1b[1;5H".to_vec(),
        ),
    ] {
        let (want_print, want_screen, want_cursor, _) = printed_at(&with, with.len());
        assert_eq!(want_print, page(["  cd", "", "", ""]), "{with:?}");
        let (_, plain_screen, plain_cursor, _) = printed_at(&without, without.len());
        assert!(want_screen == plain_screen, "the cells: {with:?}");
        assert_eq!(want_cursor, plain_cursor, "the cursor: {with:?}");
        for read in 1..=with.len() {
            let got = printed_at(&with, read);
            assert_eq!(got.0, want_print, "{read}-byte reads of {with:?}");
            assert!(got.1 == want_screen, "{read}-byte reads: the cells");
            assert_eq!(got.3, 0, "{read}-byte reads: nothing counted");
        }
        for at in 1..with.len() {
            let mut s = owned();
            let mut t = Term::new(s.storage(), &WS);
            t.set_printer(PrinterPort::Ready);
            for piece in [&with[..at], &with[at..]] {
                t.write_at(BytePos::ZERO, piece);
                t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
            }
            support::feed(&mut t, b"\x1b[i");
            let job = t
                .print_jobs()
                .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
                .next();
            assert_eq!(job.as_deref(), Some(want_print.as_str()), "split at {at}");
        }
    }
}

/// What `DECRQM` answers for ANSI mode `m`.
fn ask(t: &mut Term<'_>, m: u16) -> String {
    t.clear_reply();
    support::feed(t, format!("\x1b[{m}$p").as_bytes());
    let r = String::from_utf8_lossy(t.reply()).into_owned();
    t.clear_reply();
    r
}

/// **DECRQM answers MATM, TTM and SATM; `RIS` resets them and forgets the
/// areas, and `DECSTR` keeps both**: Table 6's reset states at power-on,
/// as 7.1 recommends, and ECMA-48 8.3.105's initial state after `RIS`;
/// DECSTR's table names neither, as it names no GATM.
#[test]
fn decrqm_answers_the_three_modes_and_ris_resets_them_and_decstr_keeps_them() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer(PrinterPort::Ready);
    for m in [15, 16, 17] {
        assert_eq!(ask(&mut t, m), format!("\x1b[{m};2$y"), "{m} at power-on");
    }
    support::feed(&mut t, b"\x1b[15;16;17h");
    for m in [15, 16, 17] {
        assert_eq!(ask(&mut t, m), format!("\x1b[{m};1$y"), "{m} set");
    }
    support::feed(&mut t, b"\x1b[!p");
    for m in [15, 16, 17] {
        assert_eq!(ask(&mut t, m), format!("\x1b[{m};1$y"), "{m} after DECSTR");
    }
    support::feed(&mut t, b"\x1bc");
    for m in [15, 16, 17] {
        assert_eq!(ask(&mut t, m), format!("\x1b[{m};2$y"), "{m} after RIS");
    }
    assert_eq!(t.unsupported_param, 0, "none counted");
    // The areas: kept across DECSTR, forgotten by RIS.
    let kept = [TWO, b"\x1b[!p\x1b[1;5H"].concat();
    prints_at_every_read(&kept, &page(["  cd", "", "", ""]));
    let gone = [TWO, b"\x1bcabcdefgh\x1b[1;5H"].concat();
    prints_at_every_read(&gone, &page(["abcdefgh", "", "", ""]));
}

/// **The areas are bounded**: a seventeenth on the screen, none of the
/// sixteen scrolled above it, is refused and counted; once they have
/// scrolled above the screen they are swept and a new one is kept.
#[test]
fn a_seventeenth_area_is_refused_and_counted_until_the_others_scroll_away() {
    let mut s = owned();
    let mut t = Term::new(s.storage(), &WS);
    t.set_printer(PrinterPort::Ready);
    for _ in 0..17 {
        support::feed(&mut t, b"\x1b[1;1H\x1bFa\x1bG");
    }
    assert_eq!(t.print_refusals().areas_refused, 1);
    support::feed(&mut t, b"\x1b[4;1H\n\n\n\n\x1b[1;1H\x1bFb\x1bG");
    assert_eq!(t.print_refusals().areas_refused, 1, "swept, then kept");
}

/// **An area pushed below the screen by a full-screen scroll down goes**:
/// RI at the top, or SD, pushes row 4's line off the bottom, and the line
/// a scroll up then brings in there is erased, so `qq` written on it is in
/// no area and case a prints the whole screen, as a region's scroll that
/// pushes an end out of its band does, on both screens. With the window's
/// top at line zero the ends move down a line; past it, after six line
/// feeds, the primary's window moves back over a line and the ends stay
/// on theirs.
#[test]
fn an_area_pushed_below_the_screen_goes() {
    let all = page(["aaa", "bbb", "ccc", "qq"]);
    for p in pushed_below_programs() {
        prints_at_every_read(&p, &all);
        let scrolled = [&b"x\x1b[4;1H\n\n\n\n\n\n"[..], &p].concat();
        prints_at_every_read(&scrolled, &all);
    }
}

/// **The same where the rows hold something past the live edge**, whose
/// scroll down takes its lines back out of line: written 20 wide and
/// narrowed to 10 at a prompt, so the first row keeps fifteen `S`, and the
/// scroll down brings that row back to the top with its text.
#[test]
fn an_area_pushed_below_a_screen_narrowed_past_its_rows_goes() {
    let primary = page(["aaaSSSSSSS", "bbb", "ccc", "qq"]);
    let alternate = page(["aaa", "bbb", "ccc", "qq"]);
    for (p, want) in pushed_below_programs()
        .into_iter()
        .zip([&primary, &alternate, &primary])
    {
        for read in 1..=p.len() {
            let pieces: Vec<&[u8]> = p.chunks(read).collect();
            assert_eq!(
                printed_narrowed(&pieces),
                *want,
                "{read}-byte reads of {p:?}"
            );
        }
        for at in 1..p.len() {
            let got = printed_narrowed(&[&p[..at], &p[at..]]);
            assert_eq!(got, *want, "{p:?} split at {at}");
        }
    }
}

/// An area on row 4, pushed below the screen and the scroll undone, then
/// `aaa` to `qq` written down the rows with the cursor left on `qq`'s:
/// RI at the top and a line feed at the bottom, the same on the alternate
/// screen, and SD then SU.
fn pushed_below_programs() -> [Vec<u8>; 3] {
    let area = b"\x1b[4;1H\x1bFxy\x1bG";
    let after = b"\x1b[1;1Haaa\x1b[2;1Hbbb\x1b[3;1Hccc\x1b[4;1Hqq";
    let ri = b"\x1b[1;1H\x1bM\x1b[4;1H\n";
    [
        [&area[..], ri, after].concat(),
        [&b"\x1b[?1049h"[..], area, ri, after].concat(),
        [&area[..], b"\x1b[T\x1b[S", after].concat(),
    ]
}

/// The print after `pieces`, each a read, on a screen of 20 columns
/// narrowed to 10 at a prompt over a row of fifteen `S`, which keeps its
/// tail past the live edge.
fn printed_narrowed(pieces: &[&[u8]]) -> String {
    const WIDE: WinSize = WinSize {
        ws_col: 20,
        ws_row: 4,
    };
    let mut s = Owned::new(&WIDE).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(s.storage(), &WIDE);
    t.set_printer(PrinterPort::Ready);
    // The prompt's mark first: every row from it on is the shell's block,
    // which the narrowing keeps whole, the tail past the edge.
    support::feed(&mut t, b"\x1b]133;A\x07SSSSSSSSSSSSSSS\x1b[4;1H");
    assert_eq!(t.resize(&WS), Resized::Ok);
    assert_eq!(
        t.row_extent(Row::ZERO).index(),
        15,
        "the control: a tail lies past the edge"
    );
    for piece in pieces {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    support::feed(&mut t, b"\x1b[i");
    let jobs: Vec<String> = t
        .print_jobs()
        .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
        .collect();
    assert_eq!(jobs.len(), 1, "one print");
    jobs[0].clone()
}

/// **An area on a row a shell's redraw gives up goes**: the block starts a
/// line above the screen, so the redraw at row 2 takes the window back a
/// line and gives up row 4, blank, where the area is; the line feed that
/// brings the row back erases it, so `qq` written there is in no area and
/// case a prints the whole screen, as after a full-screen scroll down. The
/// control first: the same area and writes without the redraw select `qq`.
#[test]
fn an_area_on_a_row_a_prompt_redraw_gives_up_goes() {
    let block =
        b"\x1b]133;A\x07p1\r\np2\r\np3\r\np4\r\np5\x1b[4;1H\x1b[K\x1bF\x1b[4;3H\x1bG\x1b[2;1H";
    let after = b"\x1b[1;1Haaa\x1b[2;1Hbbb\x1b[3;1Hccc\x1b[4;1Hqq";
    let kept = [&block[..], after].concat();
    prints_at_every_read(&kept, &page(["", "", "", "qq"]));
    let redraw = [&block[..], b"\x1b]133;A\x07\x1b[4;1H\n", after].concat();
    prints_at_every_read(&redraw, &page(["aaa", "bbb", "ccc", "qq"]));
}
