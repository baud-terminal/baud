//! `CSI 3 J` and `CSI ? 3 J`: Erase Saved Lines, and its selective form.
//!
//! `[[source: ctlseqs]]`, "CSI Ps J  Erase in Display (ED), VT100 ... Ps = 3
//! -> Erase Saved Lines, xterm", and under DECSED "Ps = 3 -> Selective Erase
//! Saved Lines, xterm". The saved lines are the history: the lines that
//! scrolled off the top of the primary screen. The screen is not among them,
//! so neither the screen nor the cursor moves.
//!
//! What the text leaves open is read from xterm (`[[source: xterm-scroll]]`,
//! `do_erase_display`'s `case 3`): it sets `savedlines` to zero, from either
//! screen -- the saved lines are the normal buffer's, and only it adds to
//! them -- and touches nothing else. Its `case 3` is the same for DECSED, so
//! xterm erases protected characters there; DECSCA's text says DECSED cannot
//! (`[[source: vt510-decsca]]`), and baud follows the text. That difference
//! is the owner's call.

use crate::support;
use baud_core::*;
use baud_domain::{CacheGeometry, Depth, ImageBudget, ImagePx, ImageScreen};

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Every line behind the screen, oldest first.
fn history(t: &Term<'_>) -> Vec<String> {
    let sb = t.scrollback();
    (0..sb.len())
        .rev()
        .map(|n| text_of(sb.line(Back::of(n)).expect("a retained line").0))
        .collect()
}

fn screen_text(t: &Term<'_>) -> Vec<String> {
    (0..t.geometry().1.index())
        .map(|y| text_of(t.row(Row::from_index(y))))
        .collect()
}

/// Every row of the ring, as the core holds it, whichever surface it is.
fn ring_text(t: &Term<'_>) -> Vec<String> {
    t.cells().chunks(t.stride().index()).map(text_of).collect()
}

fn lines(tag: &str, n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| format!("{tag} {i}\r\n").into_bytes())
        .collect()
}

/// `bytes` in reads of `n`.
fn feed_in(t: &mut Term<'_>, bytes: &[u8], n: usize) {
    for c in bytes.chunks(n) {
        support::feed(t, c);
    }
}

/// The eleven lines sixty numbered lines retire on the fifty-row corpus
/// screen: `old 0` to `old 10`. Nothing may hold one after the erase.
fn erased(l: &str) -> bool {
    (0..11).any(|i| l == format!("old {i}"))
}

/// **The history goes; the screen, the cursor and the rendition stay.**
#[test]
fn erase_saved_lines_empties_the_history_and_keeps_the_screen_and_the_cursor() {
    for n in [1, 2, 4096] {
        support::with_corpus(|t| {
            feed_in(t, &lines("old", 60), n);
            feed_in(t, b"\x1b[7;9H\x1b[1;33m", n);
            assert_eq!(t.scrollback().len(), 11, "read size {n}: the control");
            let (screen, cursor, counted) = (support::screen(t), t.cursor(), t.unsupported);
            feed_in(t, b"\x1b[3J", n);
            assert_eq!(
                t.unsupported, counted,
                "read size {n}: acted on, not counted"
            );
            assert_eq!(
                t.scrollback().len(),
                0,
                "read size {n}: the history is empty"
            );
            assert!(t.scrollback().line(Back::NEWEST).is_none(), "read size {n}");
            assert_eq!(
                t.looking_back(Rows::of_count(5)),
                Rows::ZERO,
                "read size {n}: a window scrolled back has nowhere to be"
            );
            assert_eq!(
                support::screen(t),
                screen,
                "read size {n}: the screen stays"
            );
            assert_eq!(t.cursor(), cursor, "read size {n}: the cursor stays");
            // The rendition stays too: the next character is written bold
            // yellow, as it would have been without the erase.
            feed_in(t, b"x", n);
            let c = t.row(Row::from_index(6))[8];
            let mut control = Owned::new(&support::CORPUS);
            let mut u = Term::new(control.storage(), &support::CORPUS);
            support::feed(&mut u, b"\x1b[7;9H\x1b[1;33mx");
            assert_eq!(c.style, u.row(Row::from_index(6))[8].style, "read size {n}");
            // And the history refills from the screen with nothing of the
            // erased lines among it.
            feed_in(t, b"\x1b[50;1H", n);
            feed_in(t, &lines("new", 60), n);
            let h = history(t);
            assert!(
                !h.iter().any(|l| erased(l)),
                "read size {n}: an erased line came back:\n{h:#?}"
            );
            assert_eq!(
                h[0], "old 11",
                "read size {n}: the screen's top line is next"
            );
        });
    }
}

/// **Nothing of the erased lines is left in the ring.** The history count at
/// zero makes them unreadable through the scrollback, and `Term::cells`
/// hands the whole ring to a reader that applies the rotation itself -- the
/// reason `RIS` erases the ring rather than its count. The first arm is a
/// ring that has not wrapped, the second one that has.
#[test]
fn erase_saved_lines_leaves_nothing_of_them_in_the_ring() {
    let ws = WinSize {
        ws_col: 60,
        ws_row: 20,
    };
    for (depth, n, arm) in [
        (Depth::DEFAULT, 45usize, "a ring that has not wrapped"),
        (Depth::of(20), 100, "a ring that has wrapped"),
    ] {
        let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth);
        let mut t = Term::new(owned.storage(), &ws);
        support::feed(&mut t, &lines("old", n));
        let shown: Vec<String> = screen_text(&t)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        let held = ring_text(&t).iter().filter(|l| !l.is_empty()).count();
        assert!(
            held > shown.len(),
            "{arm}: the control, the ring holds more"
        );
        support::feed(&mut t, b"\x1b[3J");
        let left: Vec<String> = ring_text(&t)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(
            left.len(),
            shown.len(),
            "{arm}: the ring kept more than the screen: {left:?}"
        );
        assert!(
            left.iter().all(|l| shown.contains(l)),
            "{arm}: the ring kept {left:?} where the screen shows {shown:?}"
        );
    }
}

/// **The rows a shrink cut below the cursor are not saved lines.** They are
/// the bottom of the screen a taller window brings back, where the saved
/// lines are behind its top -- so an erase of the saved lines empties the
/// history and a growth still brings the cut rows back. `RIS` erases both,
/// because it erases the screen they belonged to.
#[test]
fn erase_saved_lines_keeps_the_rows_a_shrink_cut_below_the_window() {
    support::with_corpus(|t| {
        support::feed(t, &lines("old", 60));
        assert_eq!(t.scrollback().len(), 11, "the control: a history");
        support::feed(t, b"\x1b[6;1H");
        support::shrink(
            t,
            &WinSize {
                ws_row: 20,
                ws_col: 200,
            },
        );
        assert_eq!(t.cursor().0.index(), 5, "the cursor kept its row");
        support::feed(t, b"\x1b[3J");
        assert_eq!(t.scrollback().len(), 0, "the saved lines are gone");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert_eq!(
            text_of(t.row(Row::from_index(29))),
            "old 40",
            "a row the shrink cut came back"
        );
        assert!(
            !history(t).iter().any(|l| erased(l)),
            "and no saved line did"
        );
    });
}

/// **From the alternate screen it empties the primary's history**, because
/// the saved lines are the primary's: xterm's `savedlines` is one count that
/// only the normal buffer adds to, and `case 3` zeroes it whichever buffer is
/// shown. The alternate screen and its cursor stay as they were, and so does
/// the primary's screen, which leaving it shows again.
#[test]
fn erase_saved_lines_on_the_alternate_screen_empties_the_primarys_history() {
    support::with_corpus(|t| {
        support::feed(t, &lines("old", 60));
        let primary = screen_text(t);
        support::feed(t, b"\x1b[?1049h\x1b[5;5Halt text");
        let (alt, cursor) = (support::screen(t), t.cursor());
        assert_eq!(
            t.scrollback().len(),
            11,
            "the control: the primary's history"
        );
        support::feed(t, b"\x1b[3J");
        assert_eq!(
            t.screen(),
            Screen::Alternate,
            "still on the alternate screen"
        );
        assert_eq!(support::screen(t), alt, "the alternate screen stays");
        assert_eq!(t.cursor(), cursor, "its cursor stays");
        assert_eq!(t.scrollback().len(), 0, "the primary's history is empty");
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(screen_text(t), primary, "the primary's screen stays");
        assert_eq!(t.scrollback().len(), 0, "and its history stays empty");
        let ring = ring_text(t);
        assert!(
            !ring.iter().any(|l| erased(l)),
            "the primary's ring kept an erased line"
        );
    });
}

/// **`CSI ? 3 J` erases the saved lines selectively**: the erasable
/// characters go and what DECSCA protected stays, as DECSED does on the
/// screen (`[[source: vt510-decsca]]`: "The selective erase control functions
/// (DECSED and DECSEL) can only erase characters defined as erasable"). The
/// lines themselves stay, as rows do under DECSED. `CSI 3 J` erases the
/// protected characters with the rest, as ED does.
#[test]
fn selective_erase_saved_lines_spares_what_decsca_protected() {
    for n in [1, 4096] {
        support::with_corpus(|t| {
            feed_in(t, b"\x1b[1\"qKEEP\x1b[0\"q gone\r\n", n);
            feed_in(t, &lines("old", 60), n);
            let before = history(t);
            assert_eq!(before.len(), 12, "read size {n}: the control");
            assert_eq!(before[0], "KEEP gone", "read size {n}: the control");
            let (screen, cursor) = (support::screen(t), t.cursor());
            feed_in(t, b"\x1b[?3J", n);
            let after = history(t);
            assert_eq!(after.len(), 12, "read size {n}: the lines stay");
            assert_eq!(after[0], "KEEP", "read size {n}: protected text stays");
            assert!(
                after[1..].iter().all(|l| l.is_empty()),
                "read size {n}: every erasable character went: {after:#?}"
            );
            assert_eq!(
                support::screen(t),
                screen,
                "read size {n}: the screen stays"
            );
            assert_eq!(t.cursor(), cursor, "read size {n}: the cursor stays");
            feed_in(t, b"\x1b[3J", n);
            assert_eq!(t.scrollback().len(), 0, "read size {n}: ED 3 takes it all");
        });
    }
}

/// The selective form on the alternate screen reaches the primary's
/// history, for the plain form's reason.
#[test]
fn selective_erase_saved_lines_on_the_alternate_screen_reaches_the_primarys() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[1\"qKEEP\x1b[0\"q gone\r\n");
        support::feed(t, &lines("old", 60));
        support::feed(t, b"\x1b[?1049h\x1b[2;2Halt");
        let alt = support::screen(t);
        support::feed(t, b"\x1b[?3J");
        assert_eq!(support::screen(t), alt, "the alternate screen stays");
        let h = history(t);
        assert_eq!(h[0], "KEEP");
        assert!(h[1..].iter().all(|l| l.is_empty()), "{h:#?}");
    });
}

/// **Only `3` is new.** ED and DECSED share their parameter with EL and
/// DECSEL, which have no `3`; widening the shared selector would have read
/// `CSI 3 K` as some erase. It and `CSI 4 J` stay counted.
#[test]
fn erase_in_line_has_no_saved_lines_and_four_is_still_counted() {
    for (bytes, what) in [
        (&b"\x1b[3K"[..], "EL 3"),
        (b"\x1b[?3K", "DECSEL 3"),
        (b"\x1b[4J", "ED 4"),
        (b"\x1b[?4J", "DECSED 4"),
    ] {
        support::with_corpus(|t| {
            support::feed(t, &lines("old", 60));
            let (screen, counted) = (support::screen(t), t.unsupported);
            support::feed(t, bytes);
            assert_eq!(t.unsupported, counted + 1, "{what} is counted");
            assert_eq!(t.scrollback().len(), 11, "{what} erased no saved line");
            assert_eq!(support::screen(t), screen, "{what} erased nothing");
        });
    }
}

/// **Where the reads fall does not change what is erased.** One program --
/// the history, a protected line, the selective form, more lines, the plain
/// form from the alternate screen and back -- at every read size up to
/// eight, against one read.
#[test]
fn the_erase_does_not_depend_on_where_the_reads_fell() {
    let mut prog = lines("old", 60);
    prog.extend_from_slice(b"\x1b[1\"qP\x1b[0\"q tail\r\n");
    prog.extend(lines("mid", 20));
    prog.extend_from_slice(b"\x1b[?3J");
    prog.extend(lines("more", 5));
    prog.extend_from_slice(b"\x1b[?1049hon alt\x1b[3J\x1b[?1049l");
    prog.extend(lines("new", 3));
    let run = |n: usize| {
        support::with_corpus(|t| {
            feed_in(t, &prog, n);
            (support::screen(t), history(t), t.cursor())
        })
    };
    let whole = run(4096);
    assert_eq!(
        whole.1.len(),
        3,
        "the control: only what scrolled after the erase is saved"
    );
    for n in 1..=8 {
        assert_eq!(run(n), whole, "read size {n}");
    }
}

/// **An image in the erased history goes with it; one on the screen
/// stays.** A placement anchors to a line, and a line the erase took is
/// one no reader can reach.
#[test]
fn an_image_in_the_erased_history_goes_and_one_on_the_screen_stays() {
    const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    let ws = support::CORPUS;
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::DEFAULT)
        .with_images(ImageBudget::of(4 * (64 * 48 + 64), 8192));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    let placed = |t: &Term<'_>| {
        t.images()
            .placements()
            .filter(|(_, p)| p.screen == ImageScreen::Primary)
            .count()
    };
    support::feed(&mut t, TINY);
    support::feed(&mut t, b"\r\n");
    support::feed(&mut t, &lines("old", 60));
    support::feed(&mut t, TINY);
    assert_eq!(
        placed(&t),
        2,
        "the control: one behind the screen, one on it"
    );
    support::feed(&mut t, b"\x1b[3J");
    assert_eq!(placed(&t), 1, "the one in the history went");
}

/// **The erasure is counted, and only what empties the saved lines moves
/// the count**: `CSI 3 J`, from either screen and with or without lines to
/// erase, and `RIS`, which erases them too. An embedder's window over the
/// history reads it, because nothing else it can read across a read says
/// the lines went: the erase scrolls nothing.
#[test]
fn the_history_erasures_count_moves_on_what_empties_the_saved_lines() {
    support::with_corpus(|t| {
        support::feed(t, &lines("old", 60));
        let (first, mark) = (t.history_erasures(), t.history_mark());
        for (bytes, what) in [
            (&b"\x1b[2J"[..], "ED 2 erases the screen"),
            (b"\x1b[?2J", "DECSED 2"),
            (b"\x1b[?3J", "DECSED 3 keeps the lines"),
            (b"\x1b[!p", "DECSTR"),
            (b"\x1b[50;1Ha\r\nb\r\n", "scrolling"),
            (
                b"\x1b[?1049h\x1b[?1049l",
                "a round trip to the alternate screen",
            ),
        ] {
            support::feed(t, bytes);
            assert_eq!(t.history_erasures(), first, "{what} moved it");
        }
        support::feed(t, b"\x1b[3J");
        let second = t.history_erasures();
        assert_ne!(second, first, "ED 3 moves it");
        assert_eq!(
            t.history_moved_since(mark),
            Moved::Retired(Rows::of_count(2)),
            "and the erase scrolled nothing: the two lines above are all the window moved"
        );
        support::feed(t, b"\x1b[3J");
        let third = t.history_erasures();
        assert_ne!(third, second, "ED 3 with nothing to erase moves it too");
        support::feed(t, b"\x1b[?1049h\x1b[3J");
        let fourth = t.history_erasures();
        assert_ne!(fourth, third, "ED 3 from the alternate screen moves it");
        support::feed(t, b"\x1bc");
        assert_ne!(t.history_erasures(), fourth, "RIS moves it");
    });
}
