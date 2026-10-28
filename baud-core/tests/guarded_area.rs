//! SPA and EPA, the guarded area, and ERM, which decides what the erasing
//! functions do to it.
//!
//! * `[[source: ecma48]]` 6.5.2.2: "The character position which is the
//!   active presentation position after receipt of SPA is the first character
//!   position of the guarded area. The character position which is the active
//!   presentation position before the receipt of EPA is the last character
//!   position of the guarded area."
//! * 7.2.4 ERM: "PROTECT: Only the contents of unprotected areas are affected
//!   by an erasure control function. ALL: The contents of protected as well as
//!   of unprotected areas are affected by an erasure control function ...
//!   Control functions affected are: EA, ECH, ED, EF, EL." 7.1: "it is
//!   recommended that the reset state of the modes be the initial state."
//! * 8.3.67 IL and 8.3.32 DL: "Any occurrences of the start or end of ... a
//!   qualified area ... in the shifted part, are also shifted."
//!
//! Each test below that pins a fork the texts leave open names it.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 20,
};

/// The screen after `bytes`, a row of live cells each.
fn run(bytes: &[u8]) -> Vec<Vec<PackedCell>> {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, bytes);
    let v = t.viewport();
    (0..usize::from(WS.ws_row))
        .map(|y| v.row(Row::from_index(y)).to_vec())
        .collect()
}

/// Which cells of row `r` (0-based) are guarded, as a string of `G` and `.`.
fn guards(g: &[Vec<PackedCell>], r: usize) -> String {
    g[r].iter()
        .map(|c| if c.flags.is_guarded() { 'G' } else { '.' })
        .collect()
}

/// The text of row `r`, a space for an empty cell.
fn text(g: &[Vec<PackedCell>], r: usize) -> String {
    g[r].iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or(' ')
        })
        .collect()
}

fn no_guard(g: &[Vec<PackedCell>]) -> bool {
    g.iter().flatten().all(|c| !c.flags.is_guarded())
}

/// **Every position from SPA's through the one before EPA**, written or not:
/// the cursor after `abc` is the last position, though nothing was written
/// there.
#[test]
fn spa_and_epa_guard_from_spa_through_the_position_before_epa() {
    let g = run(b"\x1b[1;3H\x1bVabc\x1bW");
    assert_eq!(guards(&g, 0), "..GGGG..............");
    assert!(no_guard(&g[1..]), "no other row");
    // The control: the same bytes without SPA guard nothing.
    assert!(no_guard(&run(b"\x1b[1;3Habc\x1bW")));
}

/// Under `ESC % @` the C1 forms, `96` and `97`, are the same controls.
#[test]
fn the_c1_forms_are_spa_and_epa_under_latin1() {
    let g = run(b"\x1b%@\x1b[1;3H\x96abc\x97");
    assert_eq!(guards(&g, 0), "..GGGG..............");
}

/// **The area runs in the order of the character path**: the rest of SPA's
/// row, the rows between whole, and EPA's row up to the cursor.
#[test]
fn the_area_runs_across_rows_in_the_character_path() {
    let g = run(b"\x1b[2;18H\x1bV\x1b[4;3H\x1bW");
    assert_eq!(guards(&g, 0), "....................");
    assert_eq!(guards(&g, 1), ".................GGG");
    assert_eq!(guards(&g, 2), "GGGGGGGGGGGGGGGGGGGG");
    assert_eq!(guards(&g, 3), "GGG.................");
    assert_eq!(guards(&g, 4), "....................");
}

/// **EPA before SPA's position guards the positions between them**, from
/// 6.5.2's "The order of the character positions in a line of a qualified
/// area can be the same as, or opposite to, the character path of this
/// line".
#[test]
fn epa_before_spa_guards_the_positions_between_them() {
    let g = run(b"\x1b[3;6H\x1bV\x1b[2;18H\x1bW");
    assert_eq!(guards(&g, 1), ".................GGG");
    assert_eq!(guards(&g, 2), "GGGGGG..............");
    let g = run(b"\x1b[1;9H\x1bV\x1b[1;4H\x1bW");
    assert_eq!(guards(&g, 0), "...GGGGGG...........");
}

/// EPA with no SPA open ends no area, and a second EPA ends nothing more.
#[test]
fn epa_with_no_spa_guards_nothing() {
    assert!(no_guard(&run(b"abc\x1bW")));
    let g = run(b"\x1b[1;2H\x1bVab\x1bW\x1b[3;1Hcd\x1bW");
    assert_eq!(guards(&g, 0), ".GGG................");
    assert!(no_guard(&g[1..]));
}

/// **A second SPA starts the area again**.
#[test]
fn a_second_spa_starts_the_area_again() {
    let g = run(b"\x1b[1;2H\x1bVab\x1bVcd\x1bW");
    assert_eq!(guards(&g, 0), "...GGG..............");
}

/// **Under ERM reset, the initial state, ED, EL and ECH spare a guarded
/// position**; DECSCA's protection does not save a character from them.
#[test]
fn ed_el_and_ech_spare_a_guarded_position_under_erm_reset() {
    let setup = b"0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW";
    for (name, erase) in [
        ("EL 2", &b"\x1b[2K"[..]),
        ("EL 0", b"\x1b[1;1H\x1b[K"),
        ("EL 1", b"\x1b[1;10H\x1b[1K"),
        ("ED 2", b"\x1b[2J"),
        ("ED 0", b"\x1b[1;1H\x1b[J"),
        ("ED 1", b"\x1b[2;1H\x1b[1J"),
        ("ECH", b"\x1b[1;1H\x1b[10X"),
    ] {
        let g = run(&[&setup[..], erase].concat());
        assert_eq!(text(&g, 0), "  234               ", "{name}");
        assert_eq!(guards(&g, 0), "..GGG...............", "{name}");
    }
    // The control: the same erases without the area erase the row.
    let g = run(b"0123456789\x1b[2K");
    assert_eq!(text(&g, 0), "                    ");
    // DECSCA's protection is not the guard: EL erases it.
    let g = run(b"\x1b[1\"q0123\x1b[0\"q\x1b[2K");
    assert_eq!(text(&g, 0), "                    ");
}

/// **Under ERM set, ALL, they erase a guarded position, and it stays
/// guarded**: 4.2.38, to erase is "To remove the contents from character
/// positions and leaving the resulting gap open".
#[test]
fn under_erm_set_they_erase_a_guarded_position_and_it_stays_guarded() {
    let g = run(b"0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW\x1b[6h\x1b[2K");
    assert_eq!(text(&g, 0), "                    ");
    assert_eq!(guards(&g, 0), "..GGG...............");
    // Back under reset, a character written there takes no guard (below),
    // and the erased guarded positions are still spared.
    let g =
        run(b"0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW\x1b[6h\x1b[2K\x1b[6l\x1b[1;1Habcdefg\x1b[2K");
    assert_eq!(text(&g, 0), "                    ");
}

/// **A host write into a guarded position replaces the guard**, rather than
/// the position staying guarded.
#[test]
fn a_later_write_replaces_the_guard() {
    let g = run(b"0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW\x1b[1;4HZ\x1b[2K");
    assert_eq!(text(&g, 0), "  2 4               ");
    assert_eq!(guards(&g, 0), "..G.G...............");
}

/// **DECSED and DECSEL erase a guarded, erasable character and leave its
/// guard**: only DECSCA decides them, and DEC STD 070 DECSED: "The
/// character rendition and attributes associated with each position do not
/// change". A guarded character DECSCA protected stays.
#[test]
fn decsed_and_decsel_erase_a_guarded_erasable_character_and_keep_the_guard() {
    let setup = b"01\x1b[1\"q23\x1b[0\"q456789\x1b[1;2H\x1bV\x1b[1;5H\x1bW";
    for (name, erase) in [("DECSEL", &b"\x1b[?2K"[..]), ("DECSED", b"\x1b[?2J")] {
        let g = run(&[&setup[..], erase].concat());
        assert_eq!(text(&g, 0), "  23                ", "{name}");
        assert_eq!(guards(&g, 0), ".GGGG...............", "{name}");
    }
}

/// **An erase between SPA and EPA erases**: the area's end is established
/// by EPA, so nothing is guarded before it.
#[test]
fn an_erase_between_spa_and_epa_erases() {
    let g = run(b"\x1b[1;3H\x1bVabc\x1b[2K\x1bW");
    assert_eq!(text(&g, 0), "                    ");
    assert_eq!(guards(&g, 0), "..GGGG..............");
}

/// **The start of an open area moves with its line**: IL and DL shift "the
/// start ... of a qualified area" (8.3.67, 8.3.32), and a scroll moves it with
/// the data, as IL's and DL's text has it.
#[test]
fn the_start_of_an_open_area_moves_with_its_line() {
    // IL above the start moves it down two rows.
    let g = run(b"\x1b[3;3H\x1bV\x1b[1;1H\x1b[2L\x1b[5;5H\x1bW");
    assert_eq!(guards(&g, 2), "....................");
    assert_eq!(guards(&g, 4), "..GGG...............");
    // DL above it moves it up.
    let g = run(b"\x1b[5;3H\x1bV\x1b[1;1H\x1b[2M\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
    // A scroll of the whole screen, SU and a line feed at the bottom.
    let g = run(b"\x1b[5;3H\x1bV\x1b[S\x1b[4;5H\x1bW");
    assert_eq!(guards(&g, 3), "..GGG...............");
    let g = run(b"\x1b[5;3H\x1bV\x1b[10;1H\n\n\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
    // A region's scroll moves it inside the region and not outside.
    let g = run(b"\x1b[4;8r\x1b[5;3H\x1bV\x1b[8;1H\n\x1b[4;5H\x1bW");
    assert_eq!(guards(&g, 3), "..GGG...............");
    // The same on the alternate screen, which keeps no history.
    let g = run(b"\x1b[?1049h\x1b[5;3H\x1bV\x1b[10;1H\n\n\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
    // The control: with no move the start is where SPA left it.
    let g = run(b"\x1b[5;3H\x1bV\x1b[5;5H\x1bW");
    assert_eq!(guards(&g, 4), "..GGG...............");
}

/// A start scrolled off the top is the screen's first position; a start
/// whose line DL deletes is forgotten, and a later EPA guards nothing.
#[test]
fn a_start_scrolled_off_is_the_top_and_one_deleted_is_forgotten() {
    let g = run(b"\x1b[2;3H\x1bV\x1b[5S\x1b[1;5H\x1bW");
    assert_eq!(guards(&g, 0), "GGGGG...............");
    let g = run(b"\x1b[2;3H\x1bV\x1b[1;1H\x1b[3M\x1b[1;5H\x1bW");
    assert!(no_guard(&g));
}

/// **DECSTR forgets an open SPA**, as xterm's soft reset clears the flag SPA
/// set; it leaves ERM and what is guarded.
#[test]
fn decstr_forgets_an_open_spa_and_keeps_the_rest() {
    let g = run(b"\x1b[1;3H\x1bV\x1b[!p\x1b[1;5H\x1bW");
    assert!(no_guard(&g));
    let g = run(b"0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW\x1b[6h\x1b[!p\x1b[2K");
    assert_eq!(text(&g, 0), "                    ", "ERM set survives");
    assert_eq!(guards(&g, 0), "..GGG...............");
}

/// **DECSC and DECRC neither save nor restore an open SPA**: VT510's list
/// does not name it.
#[test]
fn decsc_and_decrc_do_not_carry_an_open_spa() {
    let g = run(b"\x1b[1;3H\x1bV\x1b7\x1b[1;5H\x1bW\x1b8\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 0), "..GGG...............");
    assert!(no_guard(&g[1..]));
}

/// **A screen switch keeps the start as its screen position**.
#[test]
fn a_screen_switch_keeps_the_start_as_its_position() {
    let g = run(b"\x1b[3;3H\x1bV\x1b[?1049h\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
    let g = run(b"\x1b[?1049h\x1b[3;3H\x1bV\x1b[?1049l\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
}

/// **RIS removes the guarded areas, an open SPA and resets ERM**: 8.3.105,
/// "remove qualified areas ... set the modes into the reset state".
#[test]
fn ris_removes_the_areas_and_resets_erm() {
    let g = run(b"\x1b[1;3H\x1bVabc\x1bW\x1b[6h\x1bc\x1b[1;3HQRS\x1b[2K");
    assert!(no_guard(&g));
    assert_eq!(text(&g, 0), "                    ");
    let g = run(b"\x1b[1;3H\x1bV\x1bc\x1b[1;5H\x1bW");
    assert!(no_guard(&g));
    // ERM is back at PROTECT: an area made after the reset is spared.
    let g = run(b"\x1b[6h\x1bc0123456789\x1b[1;3H\x1bV\x1b[1;5H\x1bW\x1b[2K");
    assert_eq!(text(&g, 0), "  234               ");
}

/// SM 6 and RM 6 are ERM, acted on and not counted; DECRQM answers it.
#[test]
fn erm_is_set_reset_and_reported() {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let before = t.unsupported_param;
    support::feed(&mut t, b"\x1b[6$p");
    assert_eq!(t.reply(), b"\x1b[6;2$y", "reset at power-on");
    support::feed(&mut t, b"\x1b[6h\x1b[6$p");
    assert_eq!(t.reply(), b"\x1b[6;2$y\x1b[6;1$y");
    support::feed(&mut t, b"\x1b[6l\x1b[6$p");
    assert_eq!(t.reply(), b"\x1b[6;2$y\x1b[6;1$y\x1b[6;2$y");
    assert_eq!(t.unsupported_param, before, "neither is counted");
}

/// **A guarded position on a row that scrolls off leaves no guard on the
/// line that takes its row**: EPA raises the row's extent over every
/// position it guards, or the blank of the line coming in stops short of
/// them. The alternate screen's ring is its screen, so one line feed at the
/// bottom brings the guarded row back as the new bottom line.
#[test]
fn a_recycled_row_carries_no_guard() {
    let g = run(b"\x1b[?1049h\x1b[1;1H\x1bV\x1b[1;5H\x1bW\x1b[10;1H\n");
    assert!(no_guard(&g), "the line the scroll opened is blank");
    // The control: before the scroll the row is guarded.
    let g = run(b"\x1b[?1049h\x1b[1;1H\x1bV\x1b[1;5H\x1bW");
    assert_eq!(guards(&g, 0), "GGGGG...............");
}

/// A wide character half in the area is guarded whole.
#[test]
fn a_wide_character_is_guarded_whole() {
    let g = run("\x1b[1;3H\u{4e00}\x1b[1;3H\x1bV\x1bW".as_bytes());
    assert_eq!(guards(&g, 0), "..GG................");
    let g = run("\x1b[1;3H\u{4e00}\x1b[1;4H\x1bV\x1bW\x1b[2K".as_bytes());
    assert_eq!(guards(&g, 0), "..GG................");
    assert_eq!(text(&g, 0).chars().nth(2), Some('\u{4e00}'));
}

/// IL that pushes the start off the bottom of the band forgets it, as DL
/// above it does; a full-screen scroll down at the first line, with nothing
/// behind the window, pushes the start below the screen, and EPA takes it
/// as the screen's last position.
#[test]
fn a_start_pushed_off_the_bottom() {
    let g = run(b"\x1b[9;3H\x1bV\x1b[8;1H\x1b[3L\x1b[1;5H\x1bW");
    assert!(no_guard(&g), "IL deleted the start's line");
    let mut s = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, b"\x1b[10;3H\x1bV\x1b[T\x1b[10;1H\x1bW");
    let v = t.viewport();
    let row: String = v
        .row(Row::from_index(9))
        .iter()
        .map(|c| if c.flags.is_guarded() { 'G' } else { '.' })
        .collect();
    assert_eq!(row, "GGGGGGGGGGGGGGGGGGGG");
}

/// An open start under a top-anchored region goes back with its row when
/// the region scrolls (`Paint::guard_put_back`), with history behind the
/// window and without, and one inside the region moves up with its line.
#[test]
fn an_open_start_through_a_top_anchored_regions_scroll() {
    for depth in [Depth::NONE, Depth::DEFAULT] {
        let play = |bytes: &[u8]| {
            let mut s = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, depth);
            let mut t = Term::new(s.storage(), &WS);
            support::feed(&mut t, bytes);
            let v = t.viewport();
            (0..usize::from(WS.ws_row))
                .map(|y| v.row(Row::from_index(y)).to_vec())
                .collect::<Vec<_>>()
        };
        // The start under the region stays on its row.
        let g = play(b"\x1b[1;8r\x1b[9;3H\x1bV\x1b[8;1H\n\x1b[9;5H\x1bW");
        assert_eq!(guards(&g, 8), "..GGG...............");
        // The start inside it moves up a row with its line.
        let g = play(b"\x1b[1;8r\x1b[5;3H\x1bV\x1b[8;1H\n\x1b[4;5H\x1bW");
        assert_eq!(guards(&g, 3), "..GGG...............");
    }
}

/// A resize forgets an open SPA: its start was a line of the old geometry.
#[test]
fn a_resize_forgets_an_open_spa() {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, b"\x1b[3;3H\x1bV");
    let _ = t.resize(&WinSize {
        ws_row: 10,
        ws_col: 18,
    });
    support::feed(&mut t, b"\x1b[3;5H\x1bW");
    assert!(t.viewport().iter().all(|c| !c.flags.is_guarded()));
    // The control: without the resize the same bytes guard.
    let g = run(b"\x1b[3;3H\x1bV\x1b[3;5H\x1bW");
    assert_eq!(guards(&g, 2), "..GGG...............");
}

/// **A guarded empty position is line content to a reflow**, as a styled
/// blank is: ECMA-48 defines no reflow and guards character positions
/// (6.5.2.2), and the re-cut keeps every one of them -- a row of 20 guarded
/// empty positions narrowed to 12 columns is a line of two rows, 12 and 8
/// guarded, and widened back it is one row of 20 again. This is chosen over
/// trimming them from the line's end (which loses the guard on the
/// positions past the narrow width for good, or on all of them).
#[test]
fn a_guarded_empty_row_keeps_every_guard_through_a_reflow() {
    let mut s = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, b"\x1bV\x1b[1;20H\x1bW\x1b[2;1HX");
    let row = |t: &Term<'_>, y: usize| -> String {
        t.viewport()
            .row(Row::from_index(y))
            .iter()
            .map(|c| match (c.flags.is_guarded(), c.content.as_codepoint()) {
                (true, _) => 'G',
                (false, Some(cp)) => char::from_u32(cp.value()).unwrap_or('?'),
                (false, None) => '.',
            })
            .collect()
    };
    let _ = t.resize(&WinSize {
        ws_row: 10,
        ws_col: 12,
    });
    assert_eq!(row(&t, 0), "GGGGGGGGGGGG");
    assert_eq!(row(&t, 1), "GGGGGGGG....");
    assert_eq!(row(&t, 2), "X...........");
    let _ = t.resize(&WS);
    assert_eq!(row(&t, 0), "GGGGGGGGGGGGGGGGGGGG");
    assert_eq!(row(&t, 1), "X...................");
}

/// **SSA and ESA select nothing and are counted** (`[[source: ecma48]]`
/// 8.3.138, 8.3.47): a selected area is "eligible to be transmitted ... or
/// transferred to an auxiliary input/output device", which only a transfer
/// under SATM reads, and baud sends no screen contents and builds no SATM.
/// `ESC F`, `ESC G`, and `86` and `87` under ISO 8859-1: no cell changes,
/// the cursor stays, each counted.
#[test]
fn ssa_and_esa_select_nothing_and_are_counted() {
    for (bytes, n) in [
        (&b"ab\x1bFcd\x1bGef"[..], 2),
        (&b"\x1b%@ab\x86cd\x87ef"[..], 2),
    ] {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        support::feed(&mut t, bytes);
        assert_eq!(t.unsupported, n, "{bytes:?}");
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(6)));
        let g = run(bytes);
        assert!(text(&g, 0).starts_with("abcdef "));
        assert!(no_guard(&g));
    }
}
