//! **Which way each line runs, as the core keeps it**: SCP and SPD
//! (`[[source: ecma48]]` 8.3.111, 8.3.130) select a character path, every
//! row of a paragraph holds it, and the line rules of terminal-wg's draft
//! (`[[source: terminal-wg-bidi-escape-sequences]]`) carry it from line to
//! line. The renderer's side, what a path draws, is
//! `baud-render/tests/line_direction.rs`.

mod support;

use baud_core::*;
use baud_domain::CharacterPath::{self, Default as None_, LeftToRight, RightToLeft};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

fn paths(t: &Term<'_>, back: Rows) -> Vec<CharacterPath> {
    let vp = t.view(back);
    (0..vp.rows().index())
        .map(|y| vp.path(Row::from_index(y)))
        .collect()
}

/// **DECRQSS does not answer SCP**: no DEC or xterm text lists it among
/// the controls DECRQSS reports, so it is answered as any other unknown
/// request is, `DCS 0 $ r ST`.
#[test]
fn decrqss_answers_scp_as_invalid() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2 k\x1bP$q k\x1b\\");
        assert_eq!(t.reply(), b"\x1bP0$r\x1b\\");
    });
}

/// **A line keeps its path when IL and DL move it**, and the lines they
/// bring in take the path in force.
#[test]
fn il_and_dl_move_a_line_with_its_path() {
    support::with_term(&WS, |t| {
        support::feed(t, b"a\r\n\x1b[2 kb\x1b[1;1H\x1b[1 k");
        assert_eq!(
            paths(t, Rows::ZERO),
            [LeftToRight, RightToLeft, None_, None_]
        );
        support::feed(t, b"\x1b[L");
        assert_eq!(
            paths(t, Rows::ZERO),
            [LeftToRight, LeftToRight, RightToLeft, None_],
            "IL: the new line takes SCP 1, the two below move down"
        );
        support::feed(t, b"\x1b[2 k\x1b[2M");
        assert_eq!(
            paths(t, Rows::ZERO),
            [RightToLeft, None_, RightToLeft, RightToLeft],
            "DL: SCP 2 turns the cursor's line, the line two below moves up, \
             and the two brought in take SCP 2"
        );
    });
}

/// **An erase inside a line keeps its path**: EL and ECH leave the line
/// the paragraph it was (terminal-wg's draft, "both new paragraphs receive
/// the old one's BiDi properties").
#[test]
fn an_erase_inside_a_line_keeps_its_path() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2 kabc\x1b[1 k\r\n\x1b[1;2H\x1b[2K\x1b[X\x1b[K");
        assert_eq!(paths(t, Rows::ZERO)[0], LeftToRight);
        support::feed(t, b"\x1b[2 k\x1b[1;1H\x1b[!p\x1b[2K");
        assert_eq!(
            paths(t, Rows::ZERO)[0],
            RightToLeft,
            "DECSTR keeps a line's path"
        );
    });
}

/// **SCP on a line that continues one in the history reaches it**: every
/// row of the paragraph says the same, the history's included.
#[test]
fn scp_on_a_continued_line_reaches_its_history_rows() {
    support::with_term(&WS, |t| {
        support::feed(t, b"0123456789012345678901234567890123456789012");
        assert_eq!(t.cursor().0, Row::from_index(3));
        support::feed(t, b"\x1b[2 k");
        assert_eq!(paths(t, Rows::ZERO), [RightToLeft; 4]);
        assert_eq!(
            paths(t, Rows::of_index(1))[0],
            RightToLeft,
            "the history's row"
        );
    });
}

/// **`IND` brings in a line with the path in force, and a move down to a
/// line already there takes none** ("Cursor movement never applies modes").
#[test]
fn ind_brings_in_a_line_with_the_path_and_a_move_takes_none() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2 k\x1bD\x1bD");
        assert_eq!(paths(t, Rows::ZERO), [RightToLeft, None_, None_, None_]);
        support::feed(t, b"\x1bD\x1bD");
        assert_eq!(
            paths(t, Rows::ZERO),
            [None_, None_, None_, RightToLeft],
            "the scrolled-in line"
        );
    });
}

/// **The alternate screen's clear takes the path in force, and the
/// primary's lines keep theirs** across a visit.
#[test]
fn the_alternate_screen_takes_the_path_and_the_primary_keeps_its_own() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2 ka\r\n\x1b[?1049h");
        assert_eq!(paths(t, Rows::ZERO), [RightToLeft; 4]);
        support::feed(t, b"\x1b[1 k\x1b[?1049l");
        assert_eq!(
            paths(t, Rows::ZERO),
            [RightToLeft, RightToLeft, None_, None_]
        );
    });
}

/// **RIS returns every line, on both screens, and the path in force to
/// none**, the initial state (8.3.105).
#[test]
fn ris_clears_every_path_on_both_screens() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1049h\x1b[3 S\x1b[?1049l\x1b[3 S\x1bc");
        assert_eq!(paths(t, Rows::ZERO), [None_; 4]);
        support::feed(t, b"\x1b[?47h");
        assert_eq!(
            paths(t, Rows::ZERO),
            [None_; 4],
            "the parked alternate screen"
        );
        support::feed(t, b"\x1b[?47l\r\n\r\n");
        assert_eq!(paths(t, Rows::ZERO), [None_; 4], "and none in force");
    });
}

/// **SPD with `Ps2` 1 homes the cursor**, ECMA-48's "first character
/// position in the first line", as `CUP 1 ; 1` addresses it.
#[test]
fn spd_with_ps2_one_homes_the_cursor() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\r\n\r\nab\x1b[3;1 S");
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO));
        assert_eq!(paths(t, Rows::ZERO), [RightToLeft; 4]);
    });
}

/// **ED 1 and ED 2 end the history's newest line** where a wrap left it
/// continuing onto the screen, so the first line they erase is a paragraph
/// of its own: the draft's "the last scrolled out line also needs to change
/// its ending to hard newline in modes 1 and 2", even with the cursor on
/// the top row. ED 0 leaves it.
#[test]
fn ed_1_and_2_end_the_history_line_a_wrap_left_continuing() {
    let wrapped = |t: &Term<'_>| t.view(Rows::of_index(1)).wrapped(Row::ZERO);
    for (ed, want) in [
        (&b"\x1b[J"[..], Wrap::Continues),
        (b"\x1b[1J", Wrap::Ended),
        (b"\x1b[2J", Wrap::Ended),
        (b"\x1b[1;1H\x1b[1J", Wrap::Ended),
    ] {
        support::with_term(&WS, |t| {
            support::feed(t, b"0123456789012345678901234567890123456789012");
            assert_eq!(wrapped(t), Wrap::Continues);
            support::feed(t, ed);
            assert_eq!(wrapped(t), want, "{ed:?}");
        });
    }
}

/// **A line a Sixel image scrolls in takes the path in force**, as a line
/// feed's does, and not the one an evicted history line left on its ring
/// row: every slot of a four-line history has held a right-to-left line,
/// then SCP 1, then three one-row images at the bottom, and under
/// `?8452` three that reach the right edge. The control is the same
/// seeding followed by three line feeds.
#[test]
fn a_line_a_sixel_image_scrolls_in_takes_the_path_in_force() {
    use baud_domain::{CacheGeometry, Depth, ImageBudget, ImagePx};
    const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    const RIGHT: &[u8] = b"\x1b[?8452h\x1b[4;10H\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    for (after, what) in [
        (TINY, "a Sixel image"),
        (RIGHT, "an image to the edge under ?8452"),
        (&b"\n"[..], "a line feed"),
    ] {
        let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(4))
            .with_images(ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        // `?8452` is recognized while DA lists Sixel.
        t.set_sixel_claim(baud_domain::SixelClaim::Yes);
        support::feed(&mut t, b"\x1b[2 k");
        for _ in 0..200 {
            support::feed(&mut t, b"x\r\n");
        }
        support::feed(&mut t, b"\x1b[1 k\x1b[4;1H");
        for _ in 0..3 {
            support::feed(&mut t, after);
        }
        assert_eq!(
            paths(&t, Rows::ZERO)[1..],
            [LeftToRight; 3],
            "the lines {what} brought in"
        );
    }
}

/// **SU, SD, IL and DL end the lines either side of what they insert or
/// remove**, terminal-wg's rule for SD and SU ("wherever lines are removed
/// or inserted, we ensure that the boundaries are all hard newlines"),
/// which baud applies to IL and DL as well: the row above the band, the
/// row the moved lines end on, and the history's newest line when a
/// scroll down brings a line in at the top. A moved row that still
/// runs on into the next keeps its wrap. Every row but the last continues
/// before each.
#[test]
fn su_sd_il_and_dl_end_the_lines_either_side_of_what_they_move() {
    use Wrap::{Continues as C, Ended as E};
    let marks = |t: &Term<'_>| {
        let vp = t.view(Rows::ZERO);
        (0..4)
            .map(|y| vp.wrapped(Row::from_index(y)))
            .collect::<Vec<_>>()
    };
    for (op, want) in [
        (&b"\x1b[2;1H\x1b[L"[..], [E, E, C, E]),
        (b"\x1b[2;1H\x1b[M", [E, C, E, E]),
        (b"\x1b[2;4r\x1b[T", [E, E, C, E]),
        (b"\x1b[2;4r\x1b[S", [E, C, E, E]),
        (b"\x1b[1;3r\x1b[S", [C, E, E, E]),
        (b"\x1b[1;3r\x1b[2;1H\x1b[M", [E, E, E, E]),
    ] {
        support::with_term(&WS, |t| {
            support::feed(t, b"012345678901234567890123456789012345678");
            assert_eq!(marks(t), [C, C, C, E]);
            support::feed(t, op);
            assert_eq!(marks(t), want, "{op:?}");
        });
    }
    // SD at the top of the primary: the history's newest line, which ran
    // on into the line the scroll took back, ends.
    support::with_term(&WS, |t| {
        support::feed(t, b"01234567890123456789012345678901234567890123456789012");
        let newest = |t: &Term<'_>| t.view(Rows::of_index(1)).wrapped(Row::ZERO);
        assert_eq!(newest(t), C);
        support::feed(t, b"\x1b[T");
        assert_eq!(newest(t), E, "SD at the top");
    });
}
