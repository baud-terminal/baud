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
//! cursor once a frame. **Output that scrolls carries it with its text**,
//! as it carries an image, and a placement whose text leaves the screen is
//! gone. The rule is `placed.rs`'s.
//!
//! The corpus test is the gate the owner set: no recording in
//! `tests/recordings/` gains a trail. Its teeth are the twenty hides in the
//! nine recordings it replays -- vim's seventeen, and one each in btop,
//! `pipes.raw` and cava -- none of them straight after a move (yazi-walk's
//! 28 are not among them); the five recordings with no hide at all
//! cannot fail it under any rule decided at a hide, and are its control on
//! the count.
//!
//! The tests that feed through `each_read_size` do so at read sizes 1, 3, 7
//! and whole, and those through `every_read_size` at every size from one
//! byte to the whole.
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
/// home, and the mixed stream, where vim inherits the hidden cursor of the
/// recording before it (asciiquarium's then, `pipes.raw`'s now), gained a
/// trail to it. The helix capture is the positive control,
/// through the same counters: exactly `ge` and `gg` are new.
#[test]
fn no_recording_gains_a_trail() {
    let names = [
        ("ascii/cat-scroll.raw", 0),
        ("utf8/btop.raw", 0),
        ("ascii/git-log.raw", 0),
        ("utf8/cat-cjk.raw", 0),
        ("ascii/pipes.raw", 0),
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

/// Feed `bytes` at every read size from one byte to the whole, and hand each
/// terminal and its read size to `f`: a scroll is the same scroll however
/// the reads cut its bytes, and so is where it leaves a placement.
fn every_read_size(bytes: &[u8], f: impl Fn(&Term<'_>, usize)) {
    for chunk in 1..=bytes.len() {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&t, chunk);
    }
}

/// An `X` written at row `r`, column `c` (one-based), and the cursor moved
/// back onto it and hidden: a placement on the `X`.
fn placed_on_x(r: usize, c: usize) -> String {
    format!("\x1b[{r};{c}HX\x1b[{r};{c}H\x1b[?25l")
}

/// The character under the placement, if it is on the screen.
fn under_placement(t: &Term<'_>) -> Option<char> {
    let (r, c) = t.placed_cursor()?;
    let cell = t.viewport().row(r)[c.index()];
    let x = baud_domain::Content::codepoint(baud_domain::Codepoint::from_char('X'));
    Some(if cell.content == x { 'X' } else { '?' })
}

/// **Output that scrolls under a placed cursor carries it with its text**,
/// as it carries an image: a line feed at the bottom, `SU`, `SD` and `RI` at
/// the top, on either screen, at every read size. The `X` under it is the
/// check that it rode with its text and not some other distance.
#[test]
fn a_placed_cursor_rides_output_that_scrolls_the_screen() {
    for screen in ["", "\x1b[?1049h"] {
        let place = format!("{screen}{}", placed_on_x(10, 5));
        each_read_size(place.as_bytes(), |t| {
            assert_eq!(t.placed_cursor(), at(9, 4), "{screen:?}: the control");
        });
        for (scroll, want) in [
            ("\x1b[50;1H\n\n\n", at(6, 4)),
            ("\x1b[3S", at(6, 4)),
            ("\x1b[2T", at(11, 4)),
            ("\x1b[1;1H\x1bM", at(10, 4)),
            ("\x1b[50;1H\x1bD\x1bE", at(7, 4)),
        ] {
            let bytes = format!("{place}{scroll}");
            every_read_size(bytes.as_bytes(), |t, n| {
                assert_eq!(t.placed_cursor(), want, "{screen:?} {scroll:?} at {n}");
                assert_eq!(
                    under_placement(t),
                    Some('X'),
                    "{screen:?} {scroll:?} at {n}"
                );
            });
        }
    }
}

/// **A placement whose text scrolls off the screen is gone**, and stays
/// gone when the scroll comes back: the rows a scroll brings in are blank,
/// on the primary as well, where the line is in the history and the row the
/// window takes back over it is blanked.
#[test]
fn a_placement_scrolled_off_the_screen_is_forgotten() {
    for screen in ["", "\x1b[?1049h"] {
        let top = format!("{screen}{}", placed_on_x(2, 5));
        let bottom = format!("{screen}{}", placed_on_x(49, 5));
        for (bytes, what) in [
            (format!("{top}\x1b[50;1H\n\n\n"), "off the top"),
            (format!("{top}\x1b[3S\x1b[3T"), "off the top and back"),
            (format!("{bottom}\x1b[2T"), "off the bottom"),
            (format!("{bottom}\x1b[2T\x1b[2S"), "off the bottom and back"),
        ] {
            every_read_size(bytes.as_bytes(), |t, n| {
                assert_eq!(t.placed_cursor(), None, "{screen:?} {what} at {n}");
            });
        }
        // The control: one row short of leaving, it is still on its text.
        let bytes = format!("{top}\x1b[1S");
        every_read_size(bytes.as_bytes(), |t, n| {
            assert_eq!(t.placed_cursor(), at(0, 4), "{screen:?} at {n}");
            assert_eq!(under_placement(t), Some('X'));
        });
    }
}

/// **A region moves the placements inside it and only those**: a status
/// line's `DECSTBM 1 ; 49`, which scrolls by rotation and puts the row under
/// it back, a region in the middle that scrolls by copy, `IL` and `DL`, and
/// side margins, where only a placement between them moves. One on the
/// region's edge that the scroll deletes is forgotten with its text.
#[test]
fn a_region_moves_the_placements_inside_it() {
    for screen in ["", "\x1b[?1049h"] {
        let cases: [(String, Option<(Row, Col)>); 9] = [
            (
                format!("{}\x1b[1;49r\x1b[49;1H\n\n", placed_on_x(10, 5)),
                at(7, 4),
            ),
            (
                format!("{}\x1b[1;49r\x1b[49;1H\n\n", placed_on_x(50, 5)),
                at(49, 4),
            ),
            (format!("{}\x1b[5;20r\x1b[2S", placed_on_x(10, 5)), at(7, 4)),
            (
                format!("{}\x1b[5;20r\x1b[2S", placed_on_x(30, 5)),
                at(29, 4),
            ),
            (format!("{}\x1b[5;20r\x1b[2S", placed_on_x(5, 5)), None),
            (format!("{}\x1b[5;1H\x1b[2L", placed_on_x(10, 5)), at(11, 4)),
            (format!("{}\x1b[5;1H\x1b[2M", placed_on_x(10, 5)), at(7, 4)),
            (
                format!("{}\x1b[?69h\x1b[10;50s\x1b[2S", placed_on_x(10, 20)),
                at(7, 19),
            ),
            (
                format!("{}\x1b[?69h\x1b[10;50s\x1b[2S", placed_on_x(10, 60)),
                at(9, 59),
            ),
        ];
        for (body, want) in cases {
            let bytes = format!("{screen}{body}");
            every_read_size(bytes.as_bytes(), |t, n| {
                assert_eq!(t.placed_cursor(), want, "{screen:?} {body:?} at {n}");
                if want.is_some() {
                    assert_eq!(under_placement(t), Some('X'), "{screen:?} {body:?}");
                }
            });
        }
    }
}

/// **A placement on a row a shell's redraw gives up is gone**: the window
/// moves back over the block's rows behind it and lets go of as many blank
/// rows at the bottom (`replace_prompt_block`), and a scroll that takes
/// them in again brings them back blank. The control: the same stream
/// without the redraw keeps the placement on its row.
#[test]
fn a_placement_on_a_row_a_redraw_gives_up_is_gone() {
    let mut body = String::from("\x1b]133;A\x07");
    for _ in 0..52 {
        body += "l\r\n";
    }
    // The shell clears below, the program places the cursor on a blank row.
    body += "\x1b[45;1H\x1b[J\x1b[49;1H\x1b[?25l";
    every_read_size(body.as_bytes(), |t, n| {
        assert_eq!(t.placed_cursor(), at(48, 0), "the control at {n}");
    });
    let redrawn = format!("{body}\x1b]133;A\x07\x1b[50;1H\n\n\n");
    let kept = format!("{body}\x1b[50;1H\n\n\n");
    for chunk in [1, 7, redrawn.len()] {
        let run = |bytes: &str| {
            let mut owned = Owned::new(&support::CORPUS);
            let mut t = Term::new(owned.storage(), &support::CORPUS);
            for piece in bytes.as_bytes().chunks(chunk) {
                support::feed(&mut t, piece);
            }
            t.placed_cursor()
        };
        assert_eq!(
            run(&redrawn),
            None,
            "given up and taken in blank, at {chunk}"
        );
        assert_eq!(run(&kept), at(45, 0), "the control at {chunk}");
    }
}
