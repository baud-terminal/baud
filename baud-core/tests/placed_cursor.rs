//! Where a program put the cursor before it hid it.
//!
//! **Helix draws every block cursor itself and hides the terminal's**
//! ("all block cursors are drawn manually", [[source: helix-editor]]), but it
//! still moves the terminal cursor to the primary selection each frame and
//! then hides it ([[source: helix-tui-terminal]], `draw`: `set_cursor` then
//! `hide_cursor`). So its frames end `CUP` then `?25l`, and a trail that
//! follows only a shown cursor draws nothing on `gg` or `ge`.
//!
//! [`Term::placed_cursor`] is that position: a hide that is exactly
//! `ESC [ ? 2 5 l`, with a complete `CUP` or `HVP` straight before it, places
//! the cursor where it is, and it stays placed while the program draws with
//! the cursor hidden, until the next hide decides again. A shown cursor, a
//! screen switch, either reset and a change of geometry forget it. **Every
//! hide decides**, not only a hide of a shown cursor: helix hides a hidden
//! cursor once a frame. The rule is `placed.rs`'s.
//!
//! The corpus test is the gate the owner set: no recording in
//! `tests/recordings/` gains a trail. Its teeth are the corpus's twenty hides
//! -- vim's seventeen, and one each in btop, asciiquarium and cava -- none of
//! them straight after a move; the five recordings with no hide at all
//! cannot fail it under any rule decided at a hide, and are its control on
//! the count.
//!
//! The tests that feed through `each_read_size` do so at read sizes 1, 3, 7
//! and whole.
mod support;
use baud_core::{Owned, READ_CHUNK, Resized, Term};
use baud_domain::{Col, Row, WinSize};

fn at(r: usize, c: usize) -> Option<(Row, Col)> {
    Some((Row::from_index(r), Col::at_index(c)))
}

/// Feed `bytes` at read sizes 1, 3, 7 and whole, and hand each terminal to
/// `f`: a `CUP` and the `?25l` after it split across two reads must place the
/// cursor where one read does.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&t);
    }
}

/// One of helix's frames: text drawn with the cursor hidden, then the cursor
/// moved to the selection at row `r` and hidden.
fn frame(r: usize) -> String {
    format!("\x1b[1;1Hline 001\x1b[30;1Hstatus\x1b[{r};8H\x1b[?25l")
}

#[test]
fn a_cursor_moved_and_hidden_is_placed() {
    each_read_size(b"\x1b[3;7H\x1b[?25l", |t| {
        assert!(!t.cursor_visible(), "the control: the cursor is hidden");
        assert_eq!(t.placed_cursor(), at(2, 6), "CUP then ?25l");
    });
    each_read_size(b"\x1b[3;7f\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), at(2, 6), "HVP then ?25l");
    });
    // The longest move the window holds: twenty-six bytes, the hide's six
    // making thirty-two. Its row and column clamp to the screen.
    let long = format!("\x1b[{:0>21};1H\x1b[?25l", 3);
    each_read_size(long.as_bytes(), |t| {
        assert_eq!(long.len(), 32, "the control: the window's length");
        assert_eq!(t.placed_cursor(), at(2, 0), "a move of twenty-six bytes");
    });
}

/// **A read anywhere in the arena looks back through the tail.** An embedder
/// may place each read where it likes (`Term::write_at`), and the arena holds
/// whatever an earlier read left in front of this one: the move in one read
/// at the arena's start and the hide in the next, placed further on, must
/// find the move in the tail and not in the arena's stale bytes.
#[test]
fn a_read_anywhere_in_the_arena_looks_back_through_the_tail() {
    use baud_domain::{BytePos, ReadLen};
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let (cup, hide) = (b"\x1b[3;7H", b"\x1b[?25l");
    t.write_at(BytePos::ZERO, cup);
    t.feed(BytePos::ZERO, ReadLen::from_index(cup.len()));
    let far = BytePos::from_index(200);
    t.write_at(far, hide);
    t.feed(far, ReadLen::from_index(hide.len()));
    assert_eq!(t.placed_cursor(), at(2, 6), "the hide's read at 200");
    // The control: the same read at 200 after text, not a move, places
    // nothing -- so it is the tail being read and not a lucky arena byte.
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    t.write_at(BytePos::ZERO, b"ab");
    t.feed(BytePos::ZERO, ReadLen::from_index(2));
    t.write_at(far, hide);
    t.feed(far, ReadLen::from_index(hide.len()));
    assert_eq!(t.placed_cursor(), None, "text before the hide's read");
}

#[test]
fn every_hide_places_the_cursor_again() {
    // A hide of a cursor already hidden decides again: helix hides once a
    // frame and never shows the cursor between.
    each_read_size(b"\x1b[?25l\x1b[3;7H\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), at(2, 6), "a hide of a hidden cursor");
    });
    let two = frame(2) + &frame(23);
    each_read_size(frame(2).as_bytes(), |t| {
        assert_eq!(t.placed_cursor(), at(1, 7), "the control: the first frame");
    });
    each_read_size(two.as_bytes(), |t| {
        assert_eq!(t.placed_cursor(), at(22, 7), "ge");
    });
    each_read_size((two + &frame(2)).as_bytes(), |t| {
        assert_eq!(t.placed_cursor(), at(1, 7), "gg");
    });
}

#[test]
fn drawing_while_hidden_leaves_the_placement() {
    // The program draws the next frame with the cursor hidden, moving it all
    // over the screen: the placement is where the last frame put it, not
    // where the drawing is.
    let bytes = frame(23) + "\x1b[40;3Hdrawing\x1b[12;90H";
    each_read_size(bytes.as_bytes(), |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(11), Col::at_index(89)),
            "the control: the drawing moved the cursor"
        );
        assert_eq!(t.placed_cursor(), at(22, 7));
    });
}

#[test]
fn a_hide_after_anything_else_places_nothing() {
    each_read_size(b"\x1b[3;7Hab\x1b[?25l", |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(8)),
            "the control: the text moved the cursor"
        );
        assert_eq!(t.placed_cursor(), None, "text after the CUP");
    });
    each_read_size(b"ab\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "no CUP at all");
    });
    each_read_size(b"\x1b[3;7H\x1b[2C\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "a relative move after it");
    });
    each_read_size(b"\x1b[3;7H\n\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "a line feed after it");
    });
    // **A control sequence between them ends it**, one that leaves the cursor
    // where it was included: vim's start-up, where the erase keeps the cursor
    // at home and a test of the position alone placed it.
    each_read_size(b"\x1b[H\x1b[2J\x1b[?25l", |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(0), Col::at_index(0)),
            "the control: the cursor is where the CUP left it"
        );
        assert_eq!(t.placed_cursor(), None, "CUP ED ?25l");
    });
    each_read_size(b"\x1b[3;7H\x1b[?7l\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "a mode between them");
    });
    // Nor an `SGR`, which draws nothing: the hide is right after the move or
    // it is not, and helix sends nothing between.
    each_read_size(b"\x1b[3;7H\x1b[1;31m\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "an SGR between them");
    });
    // A hide with other modes in its sequence is not the one spelling.
    each_read_size(b"\x1b[3;7H\x1b[?25;7l", |t| {
        assert!(!t.cursor_visible(), "the control: it hid the cursor");
        assert_eq!(t.placed_cursor(), None, "CSI ? 25 ; 7 l");
    });
    // **The 8-bit `CSI` hide does not place**, under ISO 8859-1 where it is a
    // hide at all: `9B ? 2 5 l` is five bytes, and with a byte in front of it
    // the window's sixth byte back is the move's final byte -- which a check
    // of the final `l` alone would take for a hide right after a move. The
    // control is the same stream with the 7-bit hide, which places.
    each_read_size(b"\x1b%@\x1b[3;7H\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), at(2, 6), "the control: ISO 8859-1");
    });
    each_read_size(b"\x1b%@\x1b[3;7H\x9b?25l", |t| {
        assert!(!t.cursor_visible(), "the control: the 8-bit hide hid it");
        assert_eq!(t.placed_cursor(), None, "an 8-bit hide");
    });
    each_read_size(b"\x1b%@\x1b[3;7H\x07\x9b?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "an 8-bit hide a byte after a move");
    });
    // A move with a marker, or past the window, is not a move here -- at
    // every read size, so the window is the stream's and not the read's.
    each_read_size(b"\x1b[?3;7H\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "a marker");
    });
    let long = format!("\x1b[{:0>22};1H\x1b[?25l", 3);
    each_read_size(long.as_bytes(), |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(0)),
            "the control: the move took effect"
        );
        assert_eq!(t.placed_cursor(), None, "a move of twenty-seven bytes");
    });
    // A hide that does not follow a placing move clears the earlier one.
    each_read_size(b"\x1b[3;7H\x1b[?25lab\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "a second hide after text");
    });
}

/// **An `ESC` inside an open `CSI` abandons it, so the bytes after it are the
/// move they spell.** `CSI ESC [ 3 ; 7 H` is `CUP 3;7`, and the hide straight
/// after it places the cursor there. This was pinned the other way -- the
/// cursor where the text `3;7H` left it, `(0, 4)` -- while the grammar
/// consumed the `ESC` and dispatched `CSI [`
/// (`baud-core/tests/esc_abandons.rs` holds the grammar).
///
/// The same bytes after a `CUP` into the last columns were how the placement
/// test reached a cursor in pending wrap. They are a move now, so no byte
/// stream reaches the refusal of a cursor off the grid; that refusal is held
/// by `placed.rs`'s own unit test instead.
#[test]
fn a_move_whose_esc_arrived_inside_a_csi_is_a_move() {
    each_read_size(b"\x1b[\x1b[3;7H\x1b[?25l", |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(6)),
            "the control: the move ran"
        );
        assert_eq!(t.placed_cursor(), at(2, 6));
    });
    each_read_size(b"\x1b[1;197H\x1b[\x1b[3;7H\x1b[?25l", |t| {
        assert_eq!(
            t.placed_cursor(),
            at(2, 6),
            "after a move into the last columns"
        );
    });
}

#[test]
fn showing_the_cursor_forgets_the_placement() {
    each_read_size(b"\x1b[3;7H\x1b[?25l\x1b[?25h", |t| {
        assert!(t.cursor_visible());
        assert_eq!(t.placed_cursor(), None, "?25h");
    });
    // And the hide after it places nothing: its bytes follow the show's, not
    // a move's.
    each_read_size(b"\x1b[3;7H\x1b[?25l\x1b[?25h\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), None, "?25l ?25h ?25l");
    });
}

#[test]
fn a_screen_switch_forgets_the_placement() {
    for switch in ["\x1b[?1049h", "\x1b[?47h", "\x1b[?1049;7h"] {
        let bytes = format!("\x1b[3;7H\x1b[?25l{switch}");
        each_read_size(bytes.as_bytes(), |t| {
            assert_eq!(t.placed_cursor(), None, "entering with {switch:?}");
        });
    }
    for (enter, leave) in [("\x1b[?1049h", "\x1b[?1049l"), ("\x1b[?47h", "\x1b[?47l")] {
        let bytes = format!("{enter}\x1b[3;7H\x1b[?25l{leave}");
        each_read_size(bytes.as_bytes(), |t| {
            assert_eq!(t.placed_cursor(), None, "leaving with {leave:?}");
        });
        // A hide straight after the switch follows the switch's bytes, not the
        // move's, so the byte rule refuses it; the forget is what the cases
        // above hold.
        let bytes = format!("{enter}\x1b[3;7H{leave}\x1b[?25l");
        each_read_size(bytes.as_bytes(), |t| {
            assert_eq!(t.placed_cursor(), None, "a hide after {leave:?}");
        });
    }
    // The control: on the alternate screen, a program places its cursor.
    each_read_size(b"\x1b[?1049h\x1b[3;7H\x1b[?25l", |t| {
        assert_eq!(t.placed_cursor(), at(2, 6));
    });
}

#[test]
fn a_reset_forgets_the_placement() {
    // Both resets show the cursor, and a shown cursor has no placement: the
    // first half, held by the forget. The second is the byte rule: a hide
    // after a reset follows the reset's bytes, though both resets home the
    // cursor where the `CUP 1;1` left it -- a rule of positions alone would
    // place it.
    for reset in ["\x1b[!p", "\x1bc"] {
        let bytes = format!("\x1b[1;1H\x1b[?25l{reset}");
        each_read_size(bytes.as_bytes(), |t| {
            assert!(t.cursor_visible(), "the reset showed the cursor");
            assert_eq!(t.placed_cursor(), None, "{reset:?}");
        });
        let bytes = format!("\x1b[1;1H{reset}\x1b[?25l");
        each_read_size(bytes.as_bytes(), |t| {
            assert_eq!(
                t.cursor(),
                (Row::from_index(0), Col::at_index(0)),
                "the control: the cursor is where the CUP left it"
            );
            assert_eq!(t.placed_cursor(), None, "a hide after {reset:?}");
        });
    }
}

#[test]
fn a_change_of_geometry_forgets_the_placement() {
    let taller = WinSize {
        ws_row: 60,
        ws_col: 200,
    };
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[3;7H\x1b[?25l");
        assert_eq!(t.placed_cursor(), at(2, 6), "the control: placed");
        // The same geometry changes nothing, and keeps it.
        assert_eq!(t.resize(&support::CORPUS), Resized::Ok);
        assert_eq!(t.placed_cursor(), at(2, 6), "a resize to the same size");
        assert_eq!(t.resize(&taller), Resized::Ok);
        assert_eq!(t.placed_cursor(), None, "a taller screen");
        support::feed(t, b"\x1b[3;7H\x1b[?25l");
        assert_eq!(t.placed_cursor(), at(2, 6), "placed again");
        let narrower = WinSize {
            ws_row: 60,
            ws_col: 120,
        };
        assert_eq!(t.resize(&narrower), Resized::Ok);
        assert_eq!(t.placed_cursor(), None, "a narrower screen");
    });
}

/// The capture in `tests/trail/`, and what it is made of: seven hides, each
/// straight after a `CUP`, and one show.
fn helix() -> Vec<u8> {
    let bytes = std::fs::read(format!(
        "{}/../tests/trail/helix-ge-gg.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the helix capture");
    let count = |pat: &[u8]| bytes.windows(pat.len()).filter(|w| *w == pat).count();
    assert_eq!(count(b"\x1b[?25l"), 7, "the capture's hides");
    assert_eq!(count(b"\x1b[?25h"), 1, "the capture's show");
    bytes
}

/// Read sizes 1, 3 and 7, and not a whole read: the `gg` frame and the `:`
/// frame were captured a second apart and fall in one 4096-byte read of the
/// replay, where the placement is made and forgotten inside the read -- as it
/// would be for a frame drawn after both arrived.
#[test]
fn helix_places_its_cursor_at_each_selection() {
    let bytes = helix();
    for chunk in [1, 3, 7] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        let mut seen: Vec<(Row, Col)> = Vec::new();
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
            if let Some(p) = t.placed_cursor() {
                if seen.last() != Some(&p) {
                    seen.push(p);
                }
            }
        }
        let want: Vec<(Row, Col)> = [at(0, 7), at(42, 7), at(0, 7)]
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(seen, want, "read size {chunk}: the first line, ge, gg");
        // The `:` shows the cursor on the command line, which forgets it.
        assert!(t.cursor_visible(), "read size {chunk}: the command line");
        assert_eq!(t.cursor(), (Row::from_index(49), Col::at_index(1)));
        assert_eq!(t.placed_cursor(), None);
    }
}

/// **The second committed helix capture, through the rule that shipped.**
/// `tests/panels/helix.raw` is helix 25.07.1 editing a file for the panel
/// measurements, and every one of its 42 hides -- counted here -- is straight
/// after a move: each must leave a placement, at one-byte reads so every hide
/// is seen as it happens.
#[test]
fn every_hide_in_the_panels_helix_capture_places() {
    let bytes = std::fs::read(format!(
        "{}/../tests/panels/helix.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the panels helix capture");
    let hide = b"\x1b[?25l";
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let (mut hides, mut placed) = (0, 0);
    for (i, piece) in bytes.chunks(1).enumerate() {
        support::feed(&mut t, piece);
        if bytes[..=i].ends_with(hide) {
            hides += 1;
            placed += usize::from(t.placed_cursor().is_some());
        }
    }
    assert_eq!(hides, 42, "the control: the capture's hides");
    assert_eq!(placed, 42, "every hide placed");
}

/// A move of a trail's target of more than two cells on either axis: kitty's
/// start threshold, which a shorter move snaps under.
type Move = ((usize, usize), (usize, usize));

/// The long moves of a trail's target, sampled after each read of `chunk`
/// bytes and before the first, **before this change and after it**. Before,
/// a trail followed a shown cursor and kept where it last saw one while the
/// cursor was hidden. After, a hidden cursor a program placed is followed as
/// well, and one not placed keeps the target as before.
fn trails(bytes: &[u8], chunk: usize) -> (Vec<Move>, Vec<Move>, usize) {
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let (mut old, mut new) = (Vec::new(), Vec::new());
    let (mut old_at, mut new_at) = (None, None);
    let mut placements = 0;
    let mut last_placed = None;
    let step = |t: &Term<'_>, at: &mut Option<(usize, usize)>, moves: &mut Vec<Move>, placed| {
        let now = match (t.cursor_visible(), placed) {
            (true, _) => Some(t.cursor()),
            (false, true) => t.placed_cursor(),
            (false, false) => None,
        };
        let Some((r, c)) = now else { return };
        let now = (r.index(), c.index());
        if let Some(was) = *at {
            let (dr, dc) = (was.0.abs_diff(now.0), was.1.abs_diff(now.1));
            if dr > 2 || dc > 2 {
                moves.push((was, now));
            }
        }
        *at = Some(now);
    };
    step(&t, &mut old_at, &mut old, false);
    step(&t, &mut new_at, &mut new, true);
    for piece in bytes.chunks(chunk) {
        support::feed(&mut t, piece);
        let p = t.placed_cursor();
        if p.is_some() && p != last_placed {
            placements += 1;
        }
        last_placed = p;
        step(&t, &mut old_at, &mut old, false);
        step(&t, &mut new_at, &mut new, true);
    }
    (old, new, placements)
}

fn recording(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect(name)
}

/// **The owner's gate: no recording gains a trail.** Every recording, and
/// the mixed stream that leaves one program's state to the next, moves a
/// trail's target through exactly the long moves it did before, at read size
/// 1 and at a whole read -- which are not bounds of each other: a cursor
/// shown, moved and hidden inside one read is seen differently by each.
///
/// **No recording places a cursor**, and the count is pinned beside the
/// trails so a change shows -- the teeth are the corpus's twenty hides, none
/// straight after a move (see the module doc). It was one under a first rule that compared
/// positions: vim starts with `CSI H`, `CSI 2J`, `?25l`, which it placed at
/// home, and the mixed stream, where vim inherits asciiquarium's hidden
/// cursor, gained a trail to it. The helix capture is the positive control,
/// through the same counters: exactly `ge` and `gg` are new.
#[test]
fn no_recording_gains_a_trail() {
    let names = [
        ("ascii/cat-scroll.raw", 0),
        ("utf8/btop.raw", 0),
        ("ascii/git-log.raw", 0),
        ("utf8/cat-cjk.raw", 0),
        ("ascii/asciiquarium.raw", 0),
        ("utf8/vim-unicode.raw", 0),
        ("ascii/less-pager.raw", 0),
        ("utf8/less-cjk.raw", 0),
        ("utf8/cava.raw", 0),
    ];
    for chunk in [1, READ_CHUNK] {
        let mut moved = 0;
        for (name, placed) in names {
            let (old, new, placements) = trails(&recording(name), chunk);
            moved += old.len();
            assert_eq!(new, old, "{name} at read size {chunk}");
            assert_eq!(placements, placed, "{name} at read size {chunk}");
        }
        // The control: the counter sees a trail's target move at all.
        assert!(moved > 0, "read size {chunk}: no target moved anywhere");
        let mix: Vec<u8> = names[..8].iter().flat_map(|(n, _)| recording(n)).collect();
        let (old, new, placements) = trails(&mix, chunk);
        assert_eq!(new, old, "the mixed stream at read size {chunk}");
        assert_eq!(placements, 0, "the mixed stream at read size {chunk}");
    }
    let (old, new, placements) = trails(&helix(), 1);
    assert_eq!(placements, 3, "helix: the first line, ge, gg");
    let (start, ge, gg, colon) = ((0, 7), (42, 7), (0, 7), (49, 1));
    assert_eq!(
        old.last(),
        Some(&(start, colon)),
        "helix before: one trail, to the `:`"
    );
    assert_eq!(new.len(), old.len() + 2, "helix after: two more");
    assert_eq!(
        new[new.len() - 3..],
        [(start, ge), (ge, gg), (gg, colon)],
        "helix after: ge, gg, then the `:`"
    );
    // **At a whole read the start-up is a move too, and `gg` is not.** A
    // 4096-byte read ends inside helix's first frame, which it draws with the
    // cursor shown, so the last shown place before the first hide is on row
    // 40 and the first placement is a long move from it -- which a one-byte
    // read cannot see, because there the move to 1;8 is shown before it is
    // hidden. And `gg`'s placement is made and forgotten inside the read that
    // shows the cursor for the `:`.
    let (old, new, placements) = trails(&helix(), READ_CHUNK);
    assert_eq!(placements, 2, "helix at a whole read: the first line, ge");
    let drawing = (40, 11);
    assert_eq!(
        old.last(),
        Some(&(drawing, colon)),
        "before, at a whole read"
    );
    assert_eq!(new.len(), old.len() + 2, "after, at a whole read: two more");
    assert_eq!(
        new[new.len() - 3..],
        [(drawing, start), (start, ge), (ge, colon)],
        "after, at a whole read: the start-up, ge, then the `:`"
    );
}
