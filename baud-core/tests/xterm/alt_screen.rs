//! The alternate screen buffer, `CSI ? 1049`.
//!
//! `[[source: ctlseqs]]`, `?1049 h`: "Save cursor as in DECSC, xterm. After
//! saving the cursor, switch to the Alternate Screen Buffer, clearing it
//! first." And `?1049 l`: "Use Normal Screen Buffer and restore cursor as in
//! DECRC."
//!
//! Every full-screen program -- vim, less, htop, tmux -- switches on start and
//! back on exit. Without it, quitting vim does not restore the shell's
//! scrollback, which reads to a user as lost session history rather than as a
//! missing feature.
//!
//! The property that makes it worth a whole second buffer, and the one most of
//! these tests are really about: **the primary screen must come back
//! unchanged.**

use crate::support;
use baud_core::*;

struct T {
    s: Owned,
}

impl T {
    fn new() -> T {
        T {
            s: Owned::new(&support::CORPUS),
        }
    }
}

fn run(t: &mut T, bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Screen) {
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    support::feed(&mut term, bytes);
    let (r, c) = term.cursor();
    (
        support::screen(&term),
        (r.index(), c.index()),
        term.screen(),
    )
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(&mut T::new(), bytes).0
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    run(&mut T::new(), bytes).1
}

fn on_alt(bytes: &[u8]) -> bool {
    run(&mut T::new(), bytes).2 == Screen::Alternate
}

/// Switching in and out is observable.
#[test]
fn the_switch_is_observable() {
    assert!(!on_alt(b""), "power-on is the normal screen");
    assert!(on_alt(b"\x1b[?1049h"));
    assert!(!on_alt(b"\x1b[?1049h\x1b[?1049l"));
}

/// The alternate screen starts blank even though the primary had content.
///
/// `[[source: ctlseqs]]`: "switch to the Alternate Screen Buffer, clearing it
/// first."
#[test]
fn the_alt_screen_starts_cleared() {
    assert_eq!(
        grid(b"hello world\x1b[?1049h"),
        grid(b"\x1b[?1049h"),
        "nothing from the primary screen may show through"
    );
}

/// **The point of the whole feature**: the primary screen survives untouched.
#[test]
fn the_primary_screen_is_restored_intact() {
    let before = grid(b"important scrollback");
    let after = grid(b"important scrollback\x1b[?1049h junk on the alt \x1b[?1049l");
    assert_eq!(after, before, "the primary screen must come back unchanged");
}

/// Including when the alt screen scrolled, which is where a shared buffer would
/// destroy history rather than merely overwrite a line.
///
/// **Only the alternate screen scrolls here**: 900 lines through it, and three
/// on the primary, which never scrolls, so the primary's rotation is zero on
/// both sides of the trip and a restore that dropped or reset it -- either
/// way to zero -- would pass. One that left the primary under the alternate
/// screen's rotation fails: that is not zero after 900 lines, and so this
/// test catches a `View::swap_surface` that swaps no rotation.
/// `docs/record.md` ("The alternate screen: park the primary, and the copy is
/// the cheap half") describes this test as scrolling the primary sixty lines
/// and the alternate eighty-three; that was a test deleted with `baud-core2`,
/// and `a_scrolled_primary_comes_back_in_the_order_it_left` below is this
/// file's test that scrolls the primary first.
#[test]
fn scrolling_the_alt_screen_does_not_disturb_the_primary() {
    let before = grid(b"line one\r\nline two\r\nline three");
    let mut v = b"line one\r\nline two\r\nline three\x1b[?1049h".to_vec();
    // Fill and scroll the alt screen well past its bottom.
    for i in 0..900 {
        v.extend_from_slice(format!("alt row {i}\r\n").as_bytes());
    }
    v.extend_from_slice(b"\x1b[?1049l");
    assert_eq!(grid(&v), before);
}

/// How a trip's bytes reach the terminal. The phases are the primary's
/// lines, the switch in, the alternate screen's work and the switch out.
#[derive(Clone, Copy, Debug)]
enum Reads {
    /// One read ends after each phase listed and holds every phase since the
    /// last. `[3]` is the whole trip in one read, whose switches happen
    /// inside one `Paint` and swap the surfaces there and nowhere else;
    /// `[2, 3]` ends a read on the alternate screen after the primary
    /// scrolled in it, so `Term::set_rots` has to hand back the rotation the
    /// primary reached in that read.
    After(&'static [usize]),
    /// Each phase in reads of this many bytes, so reads end while the
    /// alternate screen is up and each hands the rotations back to `Term`
    /// with the screens swapped.
    Bytes(usize),
}

/// Where the cursor is after the exit.
#[derive(Clone, Copy, Debug)]
enum Exit {
    /// `?1049 l`: "restore cursor as in DECRC", so where `?1049 h` saved it.
    Restores,
    /// `?47 l`: "Use Normal Screen Buffer", and nothing about the cursor, so
    /// it stays where the alternate screen left it.
    Stays,
}

/// `count` numbered lines, `tag` and three digits each, joined by `CR LF`
/// and not ended by one, so every row of a full screen holds a line.
fn numbered(tag: char, count: usize) -> String {
    (0..count)
        .map(|i| format!("{tag}{i:03}"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Cells as text, a blank as `.`: `Content::EMPTY` is not U+0020, and a
/// projection that dropped blanks would read a short row as a whole one.
fn shown(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|x| char::from_u32(x.value()))
                .unwrap_or('.')
        })
        .collect()
}

/// The primary's history through `Term::scrollback`, newest first, each line
/// with its wrap.
fn history(t: &Term<'_>) -> Vec<(Vec<PackedCell>, Wrap)> {
    let sb = t.scrollback();
    (0..sb.len())
        .map(|n| {
            let (cells, wrap) = sb.line(Back::of(n)).expect("a retained line");
            (cells.to_vec(), wrap)
        })
        .collect()
}

/// The first screen row on which two terminals differ, as the text each
/// shows there, for a failure that says where rather than only that.
fn first_difference(a: &Term<'_>, b: &Term<'_>) -> Option<(usize, String, String)> {
    let rows = a.geometry().1.index();
    (0..rows).find_map(|y| {
        let (x, w) = (a.row(Row::from_index(y)), b.row(Row::from_index(y)));
        (x != w).then(|| (y, shown(&x[..8]), shown(&w[..8])))
    })
}

/// **A primary that has scrolled comes back in the order it left.**
///
/// Every other round trip in this file and in `vt100/scrolling.rs` leaves a
/// primary that never scrolled, so its rotation is zero on both sides, and a
/// restore that dropped or reset it (to zero) would pass them: screen row `y`
/// is physical row `y` either way. A wrong rotation that is not zero does
/// not pass: the alternate screen's, handed back in its place, fails three of
/// them (P2 and P7 in the record section named below). Here the primary
/// scrolls first -- to 24, and to an odd 33,
/// on a ring of the screen alone; to 11 and to 111, the second past the
/// alternate screen's ring of fifty, with that many lines in the history; and
/// to 57 on a ring of seventy rows it has wrapped, so the window crosses the
/// ring's end -- and the alternate screen then scrolls, writes and moves its
/// cursor, to a rotation of 12 that is none of those.
///
/// After the exit the primary is compared whole, cell for cell, with a twin
/// fed the same primary bytes that never switched: every row, the history
/// through `Term::scrollback` (read on the alternate screen as well, where it
/// is still the primary's), and the cursor. `?1049 l` restores it "as in
/// DECRC", where `?1049 h` saved it; `?47 l` says nothing about it, so it
/// stays where the alternate screen left it (`[[source: ctlseqs]]`). `?1047`
/// and `?1048` are not implemented (`docs/vt-coverage.md`'s `?47` row), so
/// they have no case. Then rows 42-50 are erased on both and the screens
/// compared again: the rows above test the order, and the rows erased test
/// that each row's extent came back with it, since an erase stops at the
/// extent it trusts.
///
/// **The reads are part of the instrument.** A trip inside one read swaps the
/// surfaces within one `Paint` and never hands a rotation back to `Term`
/// with the alternate screen up; a read that ends there does, through
/// `Term::set_rots`. So each case runs in six cuts: the whole trip in one
/// read; one read for the primary's lines, the switch and the alternate
/// screen's work, which ends on the alternate screen after the primary
/// scrolled in it; the primary alone, then the rest; a read a phase; and
/// every phase at one byte and at seven bytes a read.
///
/// Six tests elsewhere also cross the alternate screen from a scrolled
/// primary, and each fails if the exit resets the primary's rotation:
/// `a_mark_on_the_alternate_screen_after_the_primary_scrolled` and
/// `a_block_opened_on_the_alternate_screen_is_not_redrawn_on_the_primary`
/// (`semantic.rs`), `a_shrink_under_47_anchors_the_primary_on_the_shared_cursor`
/// and `a_recut_under_47_anchors_the_primary_on_the_shared_cursor`
/// (`legacy_alt_screen.rs`),
/// `erase_saved_lines_on_the_alternate_screen_empties_the_primarys_history`
/// (`xterm/erase_saved_lines.rs`) and
/// `a_resize_on_the_alternate_screen_reshapes_the_primary_too`
/// (`xterm/soft_reset_scope.rs`). None of
/// them fails when a read that ends on the alternate screen leaves `Term`
/// with the primary's rotation from before the read -- `Term::set_rots`
/// dropping it, which bites only where the primary scrolled in that read --
/// and nor does any other test in `baud-core`. This one does, at the second
/// cut (`docs/record.md`, "A scrolled primary across the alternate screen,
/// and the hand-back no test held").
///
/// The twin is held to absolute values first -- its rotation, its top and
/// bottom rows, its history line by line and its cursor -- so a case cannot
/// pass by being a different case from the one it names.
#[test]
fn a_scrolled_primary_comes_back_in_the_order_it_left() {
    // (case, history depth, primary lines, the rotation they leave, lines kept)
    let cases: &[(&str, Depth, usize, usize, usize)] = &[
        ("an even rotation, no history", Depth::NONE, 74, 24, 0),
        ("an odd rotation, no history", Depth::NONE, 83, 33, 0),
        ("history retired", Depth::DEFAULT, 61, 11, 11),
        ("past a screenful of history", Depth::DEFAULT, 161, 111, 111),
        ("a ring it has wrapped", Depth::of(20), 177, 57, 20),
    ];
    let modes: &[(&str, &[u8], &[u8], Exit)] = &[
        ("?1049", b"\x1b[?1049h", b"\x1b[?1049l", Exit::Restores),
        ("?47", b"\x1b[?47h", b"\x1b[?47l", Exit::Stays),
    ];
    let ws = support::CORPUS;
    let rows = usize::from(ws.ws_row);
    // The alternate screen's phase: sixty lines from the top scroll ten, `SU`
    // three more and `RI` at the top one back, then a region's scroll, which
    // copies and leaves the rotation, and text where the cursor is left.
    let alt = format!(
        "\x1b[1;1H{}\x1b[3S\x1b[1;1H\x1bM\x1b[5;40r\x1b[2S\x1b[r\x1b[6;10Halt\x1b[6;10H",
        numbered('A', 60)
    );
    const ERASE: &[u8] = b"\x1b[42;1H\x1b[J!";
    for &(case, depth, lines, rot, kept) in cases {
        let scrolled = lines - rows;
        let primary = format!("{}\x1b[17;23H", numbered('P', lines));
        let want = {
            let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth);
            let mut twin = Term::new(owned.storage(), &ws);
            support::feed(&mut twin, primary.as_bytes());
            assert_eq!(twin.rotation().index(), rot, "{case}: the twin's rotation");
            assert_eq!(
                shown(&twin.row(Row::ZERO)[..4]),
                format!("P{scrolled:03}"),
                "{case}: the twin's top row"
            );
            assert_eq!(
                shown(&twin.row(Row::from_index(rows - 1))[..4]),
                format!("P{:03}", lines - 1),
                "{case}: the twin's bottom row"
            );
            let (r, c) = twin.cursor();
            assert_eq!(
                (r.index(), c.index()),
                (16, 22),
                "{case}: the twin's cursor"
            );
            let want = history(&twin);
            assert_eq!(want.len(), kept, "{case}: the lines the twin keeps");
            for (n, (cells, wrap)) in want.iter().enumerate() {
                assert_eq!(
                    (shown(&cells[..4]), *wrap),
                    (format!("P{:03}", scrolled - 1 - n), Wrap::Ended),
                    "{case}: the twin's history, {n} back"
                );
            }
            want
        };
        for &(mode, enter, leave, exit) in modes {
            let phases: [&[u8]; 4] = [primary.as_bytes(), enter, alt.as_bytes(), leave];
            for reads in [
                Reads::After(&[3]),
                Reads::After(&[2, 3]),
                Reads::After(&[0, 3]),
                Reads::After(&[0, 1, 2, 3]),
                Reads::Bytes(1),
                Reads::Bytes(7),
            ] {
                let at = format!("{case}, {mode}, {reads:?}");
                let mut twin_owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth);
                let mut twin = Term::new(twin_owned.storage(), &ws);
                support::feed(&mut twin, primary.as_bytes());
                let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth);
                let mut t = Term::new(owned.storage(), &ws);
                match reads {
                    Reads::After(ends) => {
                        let mut from = 0;
                        for &to in ends {
                            let read = phases[from..=to].concat();
                            assert!(read.len() <= READ_CHUNK, "{at}: one read holds it");
                            support::feed(&mut t, &read);
                            if to == 2 {
                                on_the_alternate_screen(&t, &want, &at);
                            }
                            from = to + 1;
                        }
                    }
                    Reads::Bytes(n) => {
                        for (i, phase) in phases.iter().enumerate() {
                            for piece in phase.chunks(n) {
                                support::feed(&mut t, piece);
                            }
                            if i == 2 {
                                on_the_alternate_screen(&t, &want, &at);
                            }
                        }
                    }
                }
                assert_eq!(t.screen(), Screen::Normal, "{at}: back on the primary");
                assert_eq!(
                    first_difference(&t, &twin),
                    None,
                    "{at}: (row, what came back, what was left)"
                );
                assert!(support::screen(&t) == support::screen(&twin), "{at}");
                assert!(history(&t) == want, "{at}: the history read back");
                let (r, c) = t.cursor();
                let cursor = match exit {
                    Exit::Restores => (16, 22),
                    Exit::Stays => (5, 9),
                };
                assert_eq!((r.index(), c.index()), cursor, "{at}: the cursor");
                support::feed(&mut t, ERASE);
                support::feed(&mut twin, ERASE);
                assert_eq!(
                    first_difference(&t, &twin),
                    None,
                    "{at}: after erasing rows 42-50 on both"
                );
            }
        }
    }
}

/// The rotation the alternate screen's phase leaves: sixty lines from its top
/// row scroll ten, `SU 3` three more and `RI` at the top one back.
const ALT_ROT: usize = 12;

/// What a read that ends on the alternate screen leaves: that screen up,
/// under a rotation of its own, and the history still the primary's.
fn on_the_alternate_screen(t: &Term<'_>, want: &[(Vec<PackedCell>, Wrap)], at: &str) {
    assert_eq!(t.screen(), Screen::Alternate, "{at}: the control");
    assert_eq!(
        t.rotation().index(),
        ALT_ROT,
        "{at}: the alternate screen's own rotation, not the primary's"
    );
    assert!(
        history(t) == want,
        "{at}: the history read on the alternate screen is the primary's"
    );
}

/// The cursor is saved on the way in and restored on the way out.
#[test]
fn the_cursor_is_saved_and_restored() {
    assert_eq!(
        cursor(b"\x1b[9;20H\x1b[?1049h\x1b[1;1Hx\x1b[?1049l"),
        (8, 19),
        "back where it was before the switch"
    );
}

/// **Entering leaves the cursor where it was.** `[[source: ctlseqs]]`'s
/// `?1049 h` saves the cursor, switches and clears, and names no move;
/// xterm's set arm is `CursorSave; ToAlternate; ClearScreen`, and
/// `ClearScreen` does not home (`xterm-util.c`). baud homed here until the
/// owner's decision of 2026-10-13. Measured at every read size and every
/// split into two reads, and by the write that follows, so a cursor that
/// reads right and writes elsewhere cannot pass.
#[test]
fn entering_leaves_the_cursor() {
    let bytes: &[u8] = b"\x1b[9;20H\x1b[?1049hx";
    for (at, cells, (r, c)) in reads(bytes) {
        assert_eq!((r, c), (8, 20), "{at}: one past the x, on its row");
        assert_eq!(char_at(&cells, 8, 19), Some('x'), "{at}: x at 9;20");
        assert_eq!(char_at(&cells, 0, 0), None, "{at}: nothing at home");
    }
}

/// **`ClearScreen` resets the wrap, and so does entering.** A character in
/// the last column leaves the cursor in pending wrap; xterm's `ClearScreen`
/// calls `ResetWrap` (`xterm-util.c`), so the cursor stays in the last
/// column with nothing pending and the next character overwrites it there
/// rather than wrapping to the next row.
#[test]
fn entering_from_a_pending_wrap_drops_the_wrap() {
    let bytes: &[u8] = b"\x1b[9;200Ha\x1b[?1049hb";
    for (at, cells, (r, c)) in reads(bytes) {
        assert_eq!(
            char_at(&cells, 8, 199),
            Some('b'),
            "{at}: b in the last column"
        );
        assert_eq!(char_at(&cells, 9, 0), None, "{at}: no wrap to the next row");
        assert_eq!((r, c), (8, 199), "{at}: pending again, after the b");
    }
}

/// The cells and cursor after `bytes` fed whole, at one byte a read, and at
/// every split into two reads, each labelled.
fn reads(bytes: &[u8]) -> Vec<(String, Vec<PackedCell>, (usize, usize))> {
    let mut out = Vec::new();
    let mut take = |at: String, pieces: &[&[u8]]| {
        let mut t = T::new();
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        for p in pieces {
            support::feed(&mut term, p);
        }
        let (r, c) = term.cursor();
        out.push((at, support::screen(&term), (r.index(), c.index())));
    };
    take("whole".into(), &[bytes]);
    let ones: Vec<&[u8]> = bytes.chunks(1).collect();
    take("read size 1".into(), &ones);
    for i in 1..bytes.len() {
        take(format!("split at {i}"), &[&bytes[..i], &bytes[i..]]);
    }
    out
}

/// The character at `(row, col)` of a screen of `support::CORPUS`'s width.
fn char_at(cells: &[PackedCell], row: usize, col: usize) -> Option<char> {
    cells[row * usize::from(support::CORPUS.ws_col) + col]
        .content
        .as_codepoint()
        .and_then(|x| char::from_u32(x.value()))
}

/// A second `?1049 h` must not overwrite the saved cursor.
///
/// An application that sets the mode twice -- or a shell prompt and a pager
/// that both do -- would otherwise be restored to a position from inside the
/// alt screen instead of the one it started from.
#[test]
fn entering_twice_does_not_clobber_the_saved_cursor() {
    assert_eq!(
        cursor(b"\x1b[9;20H\x1b[?1049h\x1b[3;3H\x1b[?1049h\x1b[?1049l"),
        (8, 19)
    );
}

/// Leaving when not on the alt screen is inert, not a restore of stale state.
#[test]
fn leaving_when_not_on_the_alt_screen_is_inert() {
    assert_eq!(cursor(b"\x1b[9;20H\x1b[?1049l"), (8, 19));
    assert_eq!(grid(b"abc\x1b[?1049l"), grid(b"abc"));
}

/// The rendition follows the cursor across the switch, as DECSC/DECRC would.
#[test]
fn the_rendition_is_saved_and_restored() {
    assert_eq!(
        grid(b"\x1b[31m\x1b[?1049h\x1b[32m\x1b[?1049lx"),
        grid(b"\x1b[31mx"),
        "the red must come back, not the green set on the alt screen"
    );
}

/// DECSC/DECRC keep their own saved cursor, independent of the switch.
///
/// The switch stashes a cursor and so does DECSC; sharing one slot would make
/// an application that does both get the wrong one back.
#[test]
fn the_switch_does_not_share_decsc_state() {
    // Save at 5;5, switch (which stashes 5;5 separately), restore inside the
    // alt screen -- DECRC must find 5;5, not be confused by the switch.
    assert_eq!(cursor(b"\x1b[5;5H\x1b7\x1b[?1049h\x1b[1;1H\x1b8"), (4, 4));
}

/// RIS returns to the normal screen.
#[test]
fn ris_returns_to_the_normal_screen() {
    assert!(!on_alt(b"\x1b[?1049h\x1bc"));
}

/// DECRQM reports the alt screen as implemented, and tracks it.
#[test]
fn decrqm_reports_the_alt_screen() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    feed(&mut t, b"\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;2$y", "implemented, currently reset");
    t.clear_reply();
    feed(&mut t, b"\x1b[?1049h\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;1$y");
}

/// **What is written on the alternate screen is what it shows.**
///
/// `[[source: ctlseqs]]`, `?1049 h`: "switch to the Alternate Screen Buffer,
/// clearing it first." So `abc` written after the switch is the alternate
/// screen's whole first row -- the three characters, then blanks where the
/// primary had `hello world`. The text goes to an explicit `CSI 1;1H` rather
/// than wherever the switch leaves the cursor, and the primary has text of its
/// own, so a screen that showed the wrong buffer reads as `hello world` and
/// one that showed nothing reads as blanks, rather than either passing.
///
/// Blanks are rendered as `.` (`Content::EMPTY` is not U+0020), so each of
/// the eleven columns `hello world` held is compared, not only the three that
/// hold text.
///
/// It replaced a test that compared `baud-core2`'s `OpMode::Runs` with its
/// `Computed` on the same bytes. With one mode the two sides were one terminal
/// built twice, and that comparison could not fail.
#[test]
fn the_alt_screen_shows_what_is_written_on_it() {
    let (cells, _, screen) = run(&mut T::new(), b"hello world\x1b[?1049h\x1b[1;1Habc");
    assert_eq!(
        screen,
        Screen::Alternate,
        "the control: the switch happened"
    );
    let shown: String = cells[..11]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|x| char::from_u32(x.value()))
                .unwrap_or('.')
        })
        .collect();
    assert_eq!(
        shown, "abc........",
        "the alternate screen's first row is what was written on it"
    );
}

/// **Leaving the alternate screen brings the primary back as it was.**
///
/// The shape of `the_primary_screen_is_restored_intact`: enter, write and
/// leave once, then compare the whole screen with a terminal that never
/// switched. The name is `baud-core2`'s, where this ran in its `OpMode::Runs`;
/// it is kept because `docs/record.md` cites the test by it. Nothing scrolls
/// the primary here, so a restore that dropped the primary's rotation would
/// pass, as the record says of this test.
#[test]
fn runs_mode_restores_the_primary_screen() {
    let mut a = T::new();
    let before = run(&mut a, b"primary text").0;
    let mut b = T::new();
    let after = run(&mut b, b"primary text\x1b[?1049halt text\x1b[?1049l").0;
    assert_eq!(after, before);
}
