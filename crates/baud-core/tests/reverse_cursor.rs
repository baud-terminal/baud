//! A cursor colour equal to the text moves to the background on each `?5`
//! change.
//!
//! xterm's `ReverseVideo` does it before the swap: "if (T_COLOR(screen,
//! TEXT_CURSOR) == T_COLOR(screen, TEXT_FG)) T_COLOR(screen, TEXT_CURSOR) =
//! T_COLOR(screen, TEXT_BG)" (`[[source: xterm-scroll]]` util.c 3293-3295),
//! and `ReverseOldColors` does the same to the copy `OSC 12 ; ?` answers from
//! (`[[source: xterm-misc]]` misc.c 6418-6431). Its `TEXT_FG` is the colour
//! drawn as text at that moment, since the flip swaps what it stores, so the
//! cursor keeps the colour of the text across the flip rather than vanishing
//! into the page. Every change of `?5` calls it -- `DECSCNM`
//! (`[[source: xterm-reset]]` charproc.c 7590-7595), XTRESTORE (8452-8458)
//! and `RIS` (14507-14508) -- and baud's are all `flip_reverse_video`.
//!
//! A cursor colour nobody set is none here and is drawn in the cell's text
//! colour, which comes to the same on a cell in the default colours; on a
//! cell with an SGR foreground under `?5`, xterm's `ShowCursor` fills with
//! the moved `TEXT_CURSOR` (its `FIX_311` test, charproc.c 13788, compares
//! against X's default pixel, not the resource), where baud draws the cell's
//! own text colour -- not followed, and not tested. It stays none.

mod support;

use baud_core::*;

const FG: (u8, u8, u8) = (0xd8, 0xd8, 0xd8);
const BG: (u8, u8, u8) = (0x10, 0x10, 0x10);
const SET: (u8, u8, u8) = (0x12, 0x34, 0x56);

fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    let ws = support::CORPUS;
    for size in [1, 3, 7, bytes.len().max(1)] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_default_colors(ColorWord::from_rgb(FG), ColorWord::from_rgb(BG));
        for chunk in bytes.chunks(size) {
            support::feed(&mut t, chunk);
        }
        f(&t);
    }
}

fn cursor(t: &Term<'_>) -> Option<(u8, u8, u8)> {
    t.dynamic_color(DynamicColor::Cursor)
}

#[test]
fn a_cursor_colour_equal_to_the_text_moves_to_the_background() {
    each_read_size(b"\x1b]12;rgb:d8/d8/d8\x07\x1b[?5h\x1b]12;?\x07", |t| {
        assert_eq!(cursor(t), Some(BG));
        assert_eq!(t.reply(), b"\x1b]12;rgb:1010/1010/1010\x07");
    });
    // And back: the text shown under `?5` is the theme's background.
    each_read_size(b"\x1b]12;rgb:d8/d8/d8\x07\x1b[?5h\x1b[?5l", |t| {
        assert_eq!(cursor(t), Some(FG));
    });
}

/// The text compared is the one drawn, an `OSC 10` override included.
#[test]
fn the_text_compared_is_the_one_drawn() {
    each_read_size(
        b"\x1b]10;rgb:12/34/56\x07\x1b]12;rgb:12/34/56\x07\x1b[?5h",
        |t| assert_eq!(cursor(t), Some(BG)),
    );
}

#[test]
fn a_cursor_colour_unlike_the_text_stays() {
    each_read_size(b"\x1b]12;rgb:12/34/56\x07\x1b[?5h", |t| {
        assert_eq!(cursor(t), Some(SET));
    });
    // The control for the comparison: equal to the *background* is not
    // equal to the text.
    each_read_size(b"\x1b]12;rgb:10/10/10\x07\x1b[?5h", |t| {
        assert_eq!(cursor(t), Some(BG));
    });
}

/// What a frame drawn in the video the mode did not stay in -- a flash --
/// reads: the colour one flip would leave, the stored colour otherwise.
#[test]
fn the_other_video_reads_the_colour_a_flip_would_leave() {
    each_read_size(b"\x1b]12;rgb:d8/d8/d8\x07", |t| {
        assert_eq!(t.cursor_color_in(Video::Normal), Some(FG));
        assert_eq!(t.cursor_color_in(Video::Reverse), Some(BG));
    });
    each_read_size(b"\x1b]12;rgb:d8/d8/d8\x07\x1b[?5h", |t| {
        assert_eq!(t.cursor_color_in(Video::Reverse), Some(BG));
        assert_eq!(t.cursor_color_in(Video::Normal), Some(FG));
    });
    each_read_size(b"\x1b]12;rgb:12/34/56\x07", |t| {
        assert_eq!(t.cursor_color_in(Video::Reverse), Some(SET));
    });
}

#[test]
fn no_cursor_colour_stays_none() {
    each_read_size(b"\x1b[?5h\x1b[?5l\x1b[?5h", |t| assert_eq!(cursor(t), None));
}

/// A core no embedder told its colours: an unset cursor stays unset even
/// where the text is unknown too, and a set one stays where the page it
/// would move to is unknown -- a colour the core cannot name is not one it
/// can move to. Every embedder that draws calls `set_default_colors`.
#[test]
fn an_unknown_colour_moves_nothing() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b]11;rgb:12/34/56\x07\x1b[?5h");
        assert_eq!(cursor(t), None, "an unset cursor against an unknown text");
    });
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b]10;rgb:12/34/56\x07\x1b]12;rgb:12/34/56\x07\x1b[?5h",
        );
        assert_eq!(cursor(t), Some(SET), "a set one against an unknown page");
    });
}

/// The move is made against the text at each flip. A cursor moved to the
/// text shown under `?5` follows an `OSC 10` sent there, as xterm's
/// `ChangeColors` carries a cursor equal to the text to the new colour
/// (util.c 3062-3070), and the flip back finds it equal to the text again
/// and moves it to the page shown after, the theme's foreground; xterm's
/// drawn cursor ends there too. A cursor of its own colour keeps it.
#[test]
fn the_move_is_made_on_each_flip_against_the_text_then() {
    each_read_size(
        b"\x1b]12;rgb:d8/d8/d8\x07\x1b[?5h\x1b]10;rgb:12/34/56\x07\x1b[?5l",
        |t| assert_eq!(cursor(t), Some(FG)),
    );
    each_read_size(
        b"\x1b]12;rgb:aa/bb/cc\x07\x1b[?5h\x1b]10;rgb:12/34/56\x07\x1b[?5l",
        |t| assert_eq!(cursor(t), Some((0xaa, 0xbb, 0xcc))),
    );
}

/// A mode that does not change moves nothing: `?5h` twice is one flip.
#[test]
fn a_repeated_set_is_not_a_flip() {
    each_read_size(b"\x1b]12;rgb:d8/d8/d8\x07\x1b[?5h\x1b[?5h", |t| {
        assert_eq!(cursor(t), Some(BG));
    });
}

/// `RIS` under `?5` goes back to normal through the same rule, as xterm's
/// hard reset calls `ReverseVideo` (charproc.c 14507-14508), and then drops
/// the program's `OSC 12` with whatever the flip moved into it, where
/// xterm keeps it; XTRESTORE's flip is the same writer, and keeps it.
#[test]
fn ris_and_xtrestore_flip_through_the_rule() {
    each_read_size(b"\x1b[?5h\x1b]12;rgb:10/10/10\x07\x1bc", |t| {
        assert!(!t.reverse_video());
        assert_eq!(cursor(t), None);
    });
    each_read_size(b"\x1b[?5s\x1b[?5h\x1b]12;rgb:10/10/10\x07\x1b[?5r", |t| {
        assert!(!t.reverse_video());
        assert_eq!(cursor(t), Some(FG));
    });
}
