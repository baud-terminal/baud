//! **A pending wrap, DEC's Last Column Flag, through the controls that keep
//! it and the ones that end it.**
//!
//! `[[source: dec-std-070]]` 070-D D.6.1 (pp. D-13 and D-14): after a
//! character fills the last column "the Active Position cannot be
//! advanced", a flag is set, and the next graphic character wraps first.
//! "The Last Column Flag should also be reset by executing any control
//! which potentially changes the Active Position from the end-of-line
//! position", among them CURSOR UP, CURSOR DOWN, CURSOR BACK, BACK_SPACE,
//! ERASE_CHARACTER, ERASE IN LINE, SELECTIVE_ERASE_IN LINE, ERASE IN
//! DISPLAY and SELECTIVE_ERASE_IN DISPLAY; and "the state of the Last
//! Column Flag should be saved when a Save Cursor operation is performed,
//! and restored when a Restore Cursor operation is performed."
//!
//! What no text governs -- a window resize, the margins `?69l` and `DECSTR`
//! put back -- keeps the flag, as xterm and ghostty keep theirs, and the
//! next character wraps from the column it was pending on.
//!
//! Every fixture is fed at every read size, from one byte to the whole, and
//! each must agree; a resize falls between two reads.
mod support;
use baud_core::{Owned, Resized, Term, Wrap, WrapPending};
use baud_domain::{Row, WinSize};

const ROWS: u16 = 6;
const COLS: u16 = 20;
/// Row 3's twenty columns.
const FULL: &str = "abcdefghijklmnopqrst";
/// `?69h`, then side margins at columns 5..10 (1-based); 0-based 4..=9.
const LRM: &str = "\x1b[?69h\x1b[5;10s";

/// A step of a fixture: bytes, fed at the read size under test, or a
/// resize to rows by columns.
enum Step {
    Bytes(String),
    Resize(u16, u16),
}
use Step::{Bytes, Resize};

fn b(s: &str) -> Step {
    Bytes(s.to_string())
}

/// What a terminal holds after a fixture.
#[derive(Clone, PartialEq, Eq, Debug)]
struct State {
    rows: Vec<String>,
    marks: Vec<Wrap>,
    cursor: (usize, usize),
    pending: WrapPending,
    reply: String,
}

fn state(t: &Term<'_>) -> State {
    let vp = t.viewport();
    let (n, w) = (vp.rows().index(), vp.cols().index());
    let rows = (0..n)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..w]
                .iter()
                .map(|c| match c.content.as_codepoint() {
                    Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                    None => '.',
                })
                .collect()
        })
        .collect();
    let marks = (0..n).map(|y| vp.wrapped(Row::from_index(y))).collect();
    let (r, c) = t.cursor();
    State {
        rows,
        marks,
        cursor: (r.index(), c.index()),
        pending: t.wrap_pending(),
        reply: String::from_utf8_lossy(t.reply()).into_owned(),
    }
}

/// The state after `steps` on a 6 by 20 screen with room for 10 by 40, fed
/// at every read size from one byte to the longest step, each of which
/// must reach the state one read does.
fn run(steps: &[Step]) -> State {
    run_on(
        &WinSize {
            ws_row: ROWS,
            ws_col: COLS,
        },
        steps,
    )
}

fn run_on(ws: &WinSize, steps: &[Step]) -> State {
    let room = WinSize {
        ws_row: ws.ws_row.max(10),
        ws_col: ws.ws_col.max(40),
    };
    let longest = steps
        .iter()
        .map(|s| match s {
            Bytes(v) => v.len(),
            Resize(..) => 0,
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let mut seen: Option<State> = None;
    for read in (1..=longest).rev() {
        let mut owned = Owned::new(&room);
        let mut t = Term::new(owned.storage(), ws);
        for s in steps {
            match s {
                Bytes(v) => {
                    for piece in v.as_bytes().chunks(read) {
                        support::feed(&mut t, piece);
                    }
                }
                Resize(r, c) => {
                    let to = WinSize {
                        ws_row: *r,
                        ws_col: *c,
                    };
                    assert_eq!(t.resize(&to), Resized::Ok, "room for {r}x{c}");
                }
            }
        }
        let got = state(&t);
        match &seen {
            None => seen = Some(got),
            Some(s) => assert_eq!(*s, got, "read size {read}"),
        }
    }
    seen.expect("one read size at least")
}

/// Row `y`'s text.
fn row(s: &State, y: usize) -> &str {
    &s.rows[y]
}

/// Row `y`'s text with a space read as empty: what a selective erase
/// leaves, "a space in its own rendition", against what the others leave.
fn erased(s: &State, y: usize) -> String {
    s.rows[y].replace(' ', ".")
}

/// The fence's pending wrap: `ABCDEF` from column 5 fills it to the right
/// margin, column 10.
fn fenced() -> String {
    format!("{LRM}\x1b[3;5HABCDEF")
}

/// Row 3 filled, the wrap pending at its last column.
fn full() -> String {
    format!("\x1b[3;1H{FULL}")
}

// ---- ?69l and DECSTR ------------------------------------------------------

/// **`?69l` and `DECSTR` keep a wrap pending at a narrower right margin**:
/// the cursor still reports the margin's column, 10, and the next character
/// wraps to the next line's left margin, column 1 once the margins are the
/// page's. `reset_left_right_margins_mode()` (p. 5-30) assigns no position
/// and no flag, DECSTR "has no affect on the Active Position" (070-4 note
/// c, p. 32), and D.6.1 names neither control. Before, the column one past
/// the old margin was read as a column: CPR said 3;11 and the `X` went
/// there.
#[test]
fn declrmm_reset_and_decstr_keep_a_wrap_pending_at_the_margin() {
    for reset in ["\x1b[?69l", "\x1b[!p"] {
        let s = run(&[Bytes(format!("{}{reset}\x1b[6n", fenced()))]);
        assert_eq!(
            s.reply, "\x1b[3;10R",
            "{reset:?}: CPR names the margin's column"
        );
        assert_eq!(s.cursor, (2, 9), "{reset:?}");
        assert_eq!(s.pending, WrapPending::Yes, "{reset:?}");
        let s = run(&[Bytes(format!("{}{reset}X", fenced()))]);
        assert_eq!(
            row(&s, 2),
            "....ABCDEF..........",
            "{reset:?}: nothing overwritten"
        );
        assert_eq!(row(&s, 3), "X...................", "{reset:?}: X wrapped");
        assert_eq!(s.cursor, (3, 1), "{reset:?}");
    }
    // The control: at full width the wrap was kept already.
    let s = run(&[Bytes(format!("{}\x1b[!pX", full()))]);
    assert_eq!(row(&s, 3), "X...................");
}

/// **A wrap taken where it waited short of the line's end does not mark
/// the row it leaves as continuing**: the line ended at the column it waited
/// on, and the next character starts a line of its own, as ghostty marks
/// only a wrap at the screen's last column (`printWrap`, `[[source:
/// ghostty-d9840f3-terminal]]` 1758-1763). A wrap at the line's end is
/// marked as before. On the base this passed without a wrap at all, the
/// `X` written one past the old margin; its teeth are a wrap that marks.
#[test]
fn a_wrap_taken_short_of_the_line_end_leaves_the_row_ended() {
    let s = run(&[Bytes(format!("{}\x1b[?69lX", fenced()))]);
    assert_eq!(s.marks[2], Wrap::Ended, "waited on column 10 of 20");
    let s = run(&[Bytes(format!("{}X", full()))]);
    assert_eq!(
        s.marks[2],
        Wrap::Continues,
        "the control: at the last column"
    );
}

/// **Every move from a wrap kept so counts from the column it waits on**:
/// `HT` to the stop after it and `CUF` one column on, where the column one
/// past the old margin was read as the cursor's.
#[test]
fn a_kept_wrap_moves_from_its_column() {
    // Margins 5..8: the wrap waits on column 8, and the stop after it is 9.
    let narrow = "\x1b[?69h\x1b[5;8s\x1b[3;5HABCD\x1b[?69l";
    let s = run(&[Bytes(format!("{narrow}\tX"))]);
    assert_eq!(row(&s, 2), "....ABCDX...........", "HT to column 9");
    let s = run(&[Bytes(format!("{narrow}\x1b[CX"))]);
    assert_eq!(row(&s, 2), "....ABCDX...........", "CUF to column 9");
    let s = run(&[Bytes(format!("{narrow}\x1b[3CX"))]);
    assert_eq!(row(&s, 2), "....ABCD..X.........", "CUF 3 to column 11");
}

/// **`CUF` from a wrap kept on a column inside the right margin stops at
/// the margin**: "If the Active Position is at or inside the Right Margin
/// ... The Active Position will not move beyond the Right Margin" (CUF,
/// note 1, p. 5-45), and a pending wrap leaves the Active Position on the
/// column it waits on (D.6.1), as xterm's `CursorForward` and ghostty's
/// `cursorRight` stop at the margin. Two ways keep one there: `DECRC` under
/// margins set since the `DECSC`, and `DECRSPS` restoring a pending wrap on
/// a column inside them. Each `CUF` lands where a `CUP` to the column and
/// the same `CUF` does; before, the kept wrap read as beyond the margin and
/// went to the screen's last column.
#[test]
fn cuf_from_a_kept_wrap_inside_the_margins_stops_at_the_margin() {
    // Saved on column 10 under margins 5..10, restored under 1..15.
    let restored = format!("{}\x1b7\x1b[?69l\x1b[?69h\x1b[1;15s\x1b8", fenced());
    let s = run(&[Bytes(format!("{restored}\x1b[6n"))]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[3;10R", WrapPending::Yes)
    );
    let s = run(&[Bytes(format!("{restored}\x1b[10CX"))]);
    assert_eq!(row(&s, 2), "....ABCDEF....X.....", "CUF 10 to column 15");
    let s = run(&[b(&format!("{LRM}\x1b[1;15s\x1b[3;10H\x1b[10CX"))]);
    assert_eq!(row(&s, 2), "..............X.....", "the control: CUP 3;10");
}

/// **And from one `DECRSPS` restores inside them**: a wrap pending on
/// column 7 under margins 5..10 stops at column 10, and one past the margin,
/// on column 11, is beyond it and goes to the screen's last column, each as
/// from a `CUP` to its column.
#[test]
fn cuf_from_a_restored_wrap_inside_the_margins_stops_at_the_margin() {
    // DECRSPS restoring a wrap pending on column `c` under margins 5..10.
    let rsps = |c: u8| format!("{LRM}\x1bP1$t3;{c};1;@;@;H;0;2;@;BBBB\x1b\\");
    let s = run(&[Bytes(format!("{}\x1b[6n", rsps(7)))]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[3;7R", WrapPending::Yes)
    );
    let s = run(&[Bytes(format!("{}\x1b[10CX", rsps(7)))]);
    assert_eq!(row(&s, 2), ".........X..........", "CUF 10 to column 10");
    let s = run(&[b(&format!("{LRM}\x1b[3;7H\x1b[10CX"))]);
    assert_eq!(row(&s, 2), ".........X..........", "the control: CUP 3;7");
    // One past the margin the wrap is beyond it.
    let s = run(&[Bytes(format!("{}\x1b[10CX", rsps(11)))]);
    assert_eq!(row(&s, 2), "...................X", "CUF 10 to column 20");
    let s = run(&[b(&format!("{LRM}\x1b[3;11H\x1b[10CX"))]);
    assert_eq!(row(&s, 2), "...................X", "the control: CUP 3;11");
}

/// **DECCIR reports what DECSC saves**: a wrap kept so as pending, bit 4 of
/// Sflag, on its column, and one a write right of the fence left at the
/// screen's last column.
#[test]
fn deccir_reports_a_kept_wrap() {
    let s = run(&[Bytes(format!("{}\x1b[?69l\x1b[1$w", fenced()))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;10;1;@;@;H;"),
        "row 3, column 10, Sflag 'H' (bit 4): {:?}",
        s.reply
    );
    let s = run(&[b(&format!("{LRM}\x1b[3;15HABCDEF\x1b[1$w"))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;20;1;@;@;H;"),
        "row 3, column 20, Sflag 'H': {:?}",
        s.reply
    );
}

// ---- DECSC and DECRC ------------------------------------------------------

/// **DECSC saves a pending wrap and DECRC restores it** (D.6.1; both
/// DECCIRs, which carry "the same information saved through a save cursor",
/// have Sflag bit 4 "autowrap pending"). At full width the clamp to the
/// grid dropped it, so the `X` after `ESC 8` overwrote the `t`.
#[test]
fn decsc_and_decrc_keep_a_pending_wrap() {
    let s = run(&[Bytes(format!("{}\x1b7\x1b[1;1H\x1b8\x1b[6n", full()))]);
    assert_eq!(s.reply, "\x1b[3;20R");
    assert_eq!(s.pending, WrapPending::Yes);
    let s = run(&[Bytes(format!("{}\x1b7\x1b[1;1H\x1b8X", full()))]);
    assert_eq!(row(&s, 2), FULL, "nothing overwritten");
    assert_eq!(row(&s, 3), "X...................");
    // ?1049's save is DECSC's.
    let s = run(&[Bytes(format!("{}\x1b[?1049h\x1b[?1049lX", full()))]);
    assert_eq!(row(&s, 2), FULL);
    assert_eq!(row(&s, 3), "X...................");
    // Saved at a fence, restored after the margins went back: the wrap
    // still waits on column 10.
    let s = run(&[Bytes(format!(
        "{}\x1b7\x1b[?69l\x1b[1;1H\x1b8\x1b[6nX",
        fenced()
    ))]);
    assert_eq!(s.reply, "\x1b[3;10R");
    assert_eq!(row(&s, 2), "....ABCDEF..........");
    assert_eq!(row(&s, 3), "X...................");
}

/// **A wrap waiting at SLL's line limit is saved and restored too**, on the
/// limit, which a report names and the next character wraps from
/// (`crate::bounds`'s wait is the same Last Column Flag).
#[test]
fn a_wrap_waiting_at_the_line_limit_is_saved_and_restored() {
    let s = run(&[b("\x1b[5 Vabcde\x1b7\x1b[1;1H\x1b8\x1b[6n")]);
    assert_eq!(s.reply, "\x1b[1;5R");
    assert_eq!(s.pending, WrapPending::Yes);
    let s = run(&[b("\x1b[5 Vabcde\x1b7\x1b[1;1H\x1b8X")]);
    assert_eq!(row(&s, 0), "abcde...............");
    assert_eq!(row(&s, 1), "X...................");
}

/// **Note AE's plan test 28, on the corpus screen**: `CSI 1;200H x ESC 7
/// CSI 1;1H ESC 8 y` puts `y` at row 2, column 1, and so do `?1049` and
/// DECRSPS restoring the report DECRQPSR asked for.
#[test]
fn a_restored_pending_wrap_wraps_on_the_corpus_screen() {
    let corpus = support::CORPUS;
    let y_at_row_two = |s: &State| {
        assert!(s.rows[0].ends_with('x'), "x on the last column");
        assert!(
            s.rows[1].starts_with('y'),
            "y at row 2: {:?}",
            &s.rows[1][..4]
        );
    };
    y_at_row_two(&run_on(&corpus, &[b("\x1b[1;200Hx\x1b7\x1b[1;1H\x1b8y")]));
    y_at_row_two(&run_on(
        &corpus,
        &[b("\x1b[1;200Hx\x1b[?1049h\x1b[?1049ly")],
    ));
    // DECRSPS: the report, read back after a move.
    let report = support::with_term(&corpus, |t| {
        support::feed(t, b"\x1b[1;200Hx\x1b[1$w");
        String::from_utf8_lossy(t.reply()).into_owned()
    });
    let body = report
        .strip_prefix("\x1bP1$u")
        .and_then(|r| r.strip_suffix("\x1b\\"))
        .expect("a DECCIR");
    let restore = format!("\x1b[1;200Hx\x1b[1;1H\x1bP1$t{body}\x1b\\y");
    y_at_row_two(&run_on(&corpus, &[Bytes(restore)]));
}

/// **DECRSPS restores a pending wrap on any column** it names, as `DECRC`
/// restores a save: Sflag bit 4, "autowrap pending", on column 5 of 20
/// leaves the cursor reporting column 5 and the next character wrapping
/// to row 4. Before, bit 4 was read only on the line's last column or the
/// right margin, and the `X` went to column 5.
#[test]
fn decrsps_restores_a_pending_wrap_on_any_column() {
    let rsps = "\x1bP1$t3;5;1;@;@;H;0;2;@;BBBB\x1b\\";
    let s = run(&[Bytes(format!("{rsps}\x1b[6n"))]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[3;5R", WrapPending::Yes)
    );
    let s = run(&[Bytes(format!("{rsps}X"))]);
    assert_eq!(row(&s, 2), "....................", "row 3 untouched");
    assert_eq!(row(&s, 3), "X...................", "X wrapped");
}

/// **DECRC leaves DECAWM as it is**: `?7l ESC 7 ?7h ESC 8` leaves autowrap
/// on. 070-5's `save_cursor` (p. 5-188) saves no DECAWM, neither DECCIR has
/// a bit for it, and xterm's `CursorRestore` restores only `DECSC_FLAGS`
/// (`[[source: xterm-cursor-c]]` 461), ATTRIBUTES, ORIGIN and PROTECTED;
/// `[[source: vt510-decsc]]`'s "Wrap flag" is the pending wrap.
#[test]
fn decrc_leaves_autowrap_as_it_is() {
    let s = run(&[Bytes(format!("\x1b[?7l\x1b7\x1b[?7h\x1b8{}X", full()))]);
    assert_eq!(row(&s, 3), "X...................", "autowrap stayed on");
    let s = run(&[Bytes(format!("\x1b7\x1b[?7l\x1b8{}X", full()))]);
    assert_eq!(row(&s, 2), "abcdefghijklmnopqrsX", "and off");
}

/// `?34h`, DECRLM: written right to left, the wrap waits at the left
/// margin.
const RLM: &str = "\x1b[?34h";
/// Row 3 under DECRLM, `FULL` written from its last column to its first.
const BACK: &str = "tsrqponmlkjihgfedcba";

/// Row 3 filled under DECRLM, the wrap waiting at its first column.
fn full_leftward() -> String {
    format!("{RLM}\x1b[3;20H{FULL}")
}

/// Row 3 filled under DECRLM from the right margin to the left one, 10
/// to 5, the wrap waiting at the left margin.
fn fenced_leftward() -> String {
    format!("{LRM}{RLM}\x1b[3;10HABCDEF")
}

/// **Under DECRLM DECSC saves a wrap waiting at the left margin and DECRC
/// restores it**: D.6.1's "the state of the Last Column Flag should be
/// saved when a Save Cursor operation is performed, and restored when a
/// Restore Cursor operation is performed" (pp. D-13, D-14) names no
/// direction, and under DECRLM a character written at the left margin
/// leaves the flag set there, the next "at the right margin column of the
/// next line" (`[[source: vt510-decrlm]]`). CPR names the margin's column
/// and the next character goes to the next line's right margin, at full
/// width, under side margins and through `?1049`'s save. Before, DECRC put
/// the cursor on the margin with no wait, and the `X` overwrote the last
/// character written.
#[test]
fn decsc_and_decrc_keep_a_wrap_waiting_at_the_left_margin() {
    for (pre, cpr, filled, next) in [
        (full_leftward(), "\x1b[3;1R", BACK, "...................X"),
        (
            fenced_leftward(),
            "\x1b[3;5R",
            "....FEDCBA..........",
            ".........X..........",
        ),
    ] {
        let s = run(&[Bytes(format!("{pre}\x1b7\x1b[1;1H\x1b8\x1b[6n"))]);
        assert_eq!(s.reply, cpr);
        let s = run(&[Bytes(format!("{pre}\x1b7\x1b[1;1H\x1b8X"))]);
        assert_eq!(row(&s, 2), filled, "nothing overwritten");
        assert_eq!(row(&s, 3), next, "X at the next line's right margin");
        let s = run(&[Bytes(format!("{pre}\x1b[?1049h\x1b[?1049lX"))]);
        assert_eq!((row(&s, 2), row(&s, 3)), (filled, next), "?1049");
        // Saved again after a restore, it is still the wait.
        let s = run(&[Bytes(format!("{pre}\x1b7\x1b8\x1b7\x1b[H\x1b8X"))]);
        assert_eq!((row(&s, 2), row(&s, 3)), (filled, next), "saved twice");
    }
}

/// **A save holds the flag only where it was set**: with DECAWM reset a
/// write at the left margin sets no Last Column Flag (D.6.1, "When Auto
/// Wrap Mode is set, however, writing a character into the last position
/// in a line causes a special flag ... to be set"), so after `?7h` and
/// DECRC the `X` overwrites the margin. **And with DECRLM reset since the
/// save it comes back a column**, as `?34l` leaves a wrap waiting there,
/// and the `X` is written on it left to right.
#[test]
fn a_saved_left_margin_wait_needs_autowrap_and_decrlm() {
    let s = run(&[Bytes(format!(
        "\x1b[?7l{}\x1b7\x1b[?7h\x1b[1;1H\x1b8X",
        full_leftward()
    ))]);
    assert_eq!(row(&s, 2), "Xsrqponmlkjihgfedcba", "?7l at the save");
    assert_eq!(row(&s, 3), "....................");
    let s = run(&[Bytes(format!(
        "{}\x1b7\x1b[?34l\x1b[1;1H\x1b8X",
        full_leftward()
    ))]);
    assert_eq!(row(&s, 2), "Xsrqponmlkjihgfedcba", "?34l since the save");
    assert_eq!(row(&s, 3), "....................");
    // Nothing pending there, so DECCIR's Sflag is clear.
    let s = run(&[Bytes(format!(
        "{}\x1b7\x1b[?34l\x1b[1;1H\x1b8\x1b[1$w",
        full_leftward()
    ))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;1;1;@;@;@;"),
        "Sflag '@': {:?}",
        s.reply
    );
}

/// **DECCIR reports the wait at the left margin, and DECRSPS restores it
/// under DECRLM**: Sflag bit 4, "autowrap pending", is the one Last Column
/// Flag DECSC saves, whichever way the line is written; read back under
/// DECRLM it waits at the column restored, and the `X` goes to the next
/// line's right margin.
#[test]
fn deccir_and_decrsps_carry_the_wait_at_the_left_margin() {
    let s = run(&[Bytes(format!("{}\x1b[1$w", full_leftward()))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;1;1;@;@;H;"),
        "row 3, column 1, Sflag 'H' (bit 4): {:?}",
        s.reply
    );
    let s = run(&[Bytes(format!("{}\x1b[1$w", fenced_leftward()))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;5;1;@;@;H;"),
        "row 3, column 5, Sflag 'H': {:?}",
        s.reply
    );
    let rsps = "\x1bP1$t3;1;1;@;@;H;0;2;@;BBBB\x1b\\";
    let s = run(&[Bytes(format!("{RLM}{rsps}X"))]);
    assert_eq!(row(&s, 2), "....................", "row 3 untouched");
    assert_eq!(row(&s, 3), "...................X", "X wrapped leftward");
}

// ---- BS and CUB -----------------------------------------------------------

/// **BS and CUB count from the column the wrap waits on**: `BS` and `CUB 1`
/// land on column 19, `CUB 2` on 18, and at a fence on column 9. BS (p.
/// 5-63) "moves the Active Position backward ... by one column", CUB (p.
/// 5-47) "backward by n columns", from the Active Position, the last column
/// (D.6.1), as xterm's `CursorBack` does without `?45` (`[[source:
/// xterm-cursor-c]]` 152-158) and ghostty's. Before, the wrap took the
/// first step and the `X` overwrote the `t`.
#[test]
fn bs_and_cub_count_from_the_last_column() {
    for (tail, want) in [
        ("\x08", "abcdefghijklmnopqrXt"),
        ("\x1b[D", "abcdefghijklmnopqrXt"),
        ("\x1b[2D", "abcdefghijklmnopqXst"),
    ] {
        let s = run(&[Bytes(format!("{}{tail}X", full()))]);
        assert_eq!(row(&s, 2), want, "{tail:?}");
    }
    let s = run(&[Bytes(format!("{}\x08\x1b[6n", full()))]);
    assert_eq!(s.reply, "\x1b[3;19R");
    for (tail, want) in [
        ("\x08", "....ABCDXF.........."),
        ("\x1b[D", "....ABCDXF.........."),
        ("\x1b[2D", "....ABCXEF.........."),
    ] {
        let s = run(&[Bytes(format!("{}{tail}X", fenced()))]);
        assert_eq!(row(&s, 2), want, "at the fence, {tail:?}");
    }
    // A wrap kept by `?69l` counts the same way.
    let s = run(&[Bytes(format!("{}\x1b[?69l\x08X", fenced()))]);
    assert_eq!(row(&s, 2), "....ABCDXF..........");
}

/// **Under `?45` or `?1045` with `?7` the wrap is still the first step**,
/// xterm's `if ((rev || rev2) && screen->do_wrap) --count`
/// (`[[source: xterm-cursor-c]]` 153): `BS` stays on the last column.
/// Without `?7` neither mode acts, and the count is from the last column.
#[test]
fn under_reverse_wrap_the_wrap_is_the_first_step() {
    for mode in ["\x1b[?45h", "\x1b[?1045h"] {
        let s = run(&[Bytes(format!("{mode}{}\x08X", full()))]);
        assert_eq!(row(&s, 2), "abcdefghijklmnopqrsX", "{mode:?}");
        let s = run(&[Bytes(format!("{mode}{}\x1b[2DX", full()))]);
        assert_eq!(row(&s, 2), "abcdefghijklmnopqrXt", "{mode:?} CUB 2");
    }
    let s = run(&[Bytes(format!("\x1b[?45h{}\x1b[?7l\x08X", full()))]);
    assert_eq!(row(&s, 2), "abcdefghijklmnopqrXt", "?45 without ?7");
}

// ---- the erases -----------------------------------------------------------

/// **EL 0, ECH, ED 0, DECSEL 0 and DECSED 0 erase from the last column and
/// end the wrap**: "from the Active Column to the end of the Active Line
/// (inclusive)" (EL, p. 5-154), the Active Column the last (D.6.1), and the
/// erases among the controls that reset the flag (D.6.1; 070-5's revision
/// item 5). xterm's `ClearRight`: "with the right part cleared, we can't be
/// wrapping" (`[[source: xterm-scroll]]` 2311-2314). Before, the erase
/// began one past the line and nothing went.
#[test]
fn the_erases_from_the_cursor_start_on_the_last_column_and_end_the_wrap() {
    for erase in ["\x1b[K", "\x1b[X", "\x1b[J", "\x1b[?K", "\x1b[?J"] {
        let s = run(&[Bytes(format!("{}{erase}\x1b[6n", full()))]);
        assert_eq!(s.reply, "\x1b[3;20R", "{erase:?}");
        assert_eq!(s.pending, WrapPending::No, "{erase:?}");
        assert_eq!(
            erased(&s, 2),
            "abcdefghijklmnopqrs.",
            "{erase:?}: the t went"
        );
        let s = run(&[Bytes(format!("{}{erase}X", full()))]);
        assert_eq!(row(&s, 2), "abcdefghijklmnopqrsX", "{erase:?}");
        assert_eq!(row(&s, 3), "....................", "{erase:?}");
    }
    let s = run(&[Bytes(format!("{}\x1b[KX", fenced()))]);
    assert_eq!(row(&s, 2), "....ABCDEX..........", "at the fence");
}

/// **The erases from the cursor at a pending wrap end the row's soft
/// wrap**: a row a wrap had marked, written to its end again, no longer
/// continues onto the next once its last column is erased, as xterm's
/// `ClearRight`, which serves `EL 0`, `ED 0` and `ECH`, clears it, "with
/// the right part cleared, we can't be wrapping" (`LineClrWrapped`,
/// `[[source: xterm-scroll]]` 2311-2312).
#[test]
fn el_and_ed_from_a_pending_wrap_end_the_rows_soft_wrap() {
    for erase in ["\x1b[K", "\x1b[J", "\x1b[X", "\x1b[?K", "\x1b[?J"] {
        let s = run(&[Bytes(format!("{}X\x1b[3;20Ht{erase}", full()))]);
        assert_eq!(s.marks[2], Wrap::Ended, "{erase:?}");
    }
    let s = run(&[Bytes(format!("{}X\x1b[3;20Ht", full()))]);
    assert_eq!(s.marks[2], Wrap::Continues, "the control");
}

/// **EL 1, EL 2, DECSEL 1 and DECSEL 2 end the wrap**: D.6.1 lists ERASE
/// IN LINE and SELECTIVE_ERASE_IN LINE whole, xterm's `ClearLeft` and
/// `ClearLine` reach `ClearInLine2`'s `ResetWrap` (`[[source:
/// xterm-scroll]]` 2223), and ghostty's `eraseLine` resets it. The row is
/// emptied through its last column and the `X` lands there.
#[test]
fn the_whole_line_erases_end_the_wrap() {
    for erase in ["\x1b[1K", "\x1b[2K", "\x1b[?1K", "\x1b[?2K"] {
        let s = run(&[Bytes(format!("{}{erase}X", full()))]);
        assert_eq!(erased(&s, 2), "...................X", "{erase:?}");
        assert_eq!(row(&s, 3), "....................", "{erase:?}");
    }
}

/// **ED 1, ED 2, DECSED 1 and DECSED 2 end the wrap**: D.6.1 lists ERASE
/// IN DISPLAY and SELECTIVE_ERASE_IN DISPLAY whole, xterm's `ClearAbove`
/// reaches `ResetWrap` through `ClearLeft` and `ClearScreen` calls it
/// (`[[source: xterm-scroll]]` 2223, 2367), and ghostty's `eraseDisplay`
/// resets it for `.above` and `.complete` (`[[source:
/// ghostty-d9840f3-terminal]]` 3571-3672). `CSI 3 J` erases the history
/// alone and keeps it.
#[test]
fn the_whole_display_erases_end_the_wrap() {
    for erase in ["\x1b[1J", "\x1b[2J", "\x1b[?1J", "\x1b[?2J"] {
        let s = run(&[Bytes(format!("{}{erase}X", full()))]);
        assert_eq!(erased(&s, 2), "...................X", "{erase:?}");
        assert_eq!(row(&s, 3), "....................", "{erase:?}");
    }
    let s = run(&[Bytes(format!("{}\x1b[3JX", full()))]);
    assert_eq!(row(&s, 3), "X...................", "ED 3 keeps it");
}

// ---- CUU and CUD ----------------------------------------------------------

/// **CUU and CUD end the wrap**: D.6.1 lists CURSOR UP and CURSOR DOWN, and
/// xterm's `CursorUp` and `CursorDown` call `ResetWrap` (`[[source:
/// xterm-cursor-c]]` 263, 284). Before, the wrap survived the move, and the
/// `X` after `CUU` wrapped back onto row 3's `a`.
#[test]
fn cuu_and_cud_end_the_wrap() {
    let s = run(&[Bytes(format!("{}\x1b[AX", full()))]);
    assert_eq!(row(&s, 1), "...................X");
    assert_eq!(row(&s, 2), FULL, "row 3 untouched");
    let s = run(&[Bytes(format!("{}\x1b[BX", full()))]);
    assert_eq!(row(&s, 3), "...................X");
    assert_eq!(row(&s, 4), "....................");
}

// ---- a resize -------------------------------------------------------------

/// **A resize keeps a pending wrap.** No text governs a window resize;
/// xterm's `ScreenResize` never touches `do_wrap` and ghostty keeps its
/// flag. At the same width -- a height change, or the same size again --
/// nothing moves and the `X` wraps; before, every resize ended the wrap and
/// the `X` overwrote the `t`.
#[test]
fn a_resize_at_the_same_width_keeps_the_wrap() {
    for (rows, cols) in [(8, 20), (6, 20), (4, 20)] {
        let s = run(&[Bytes(full()), Resize(rows, cols), b("X")]);
        assert_eq!(row(&s, 2), FULL, "{rows}x{cols}");
        assert_eq!(row(&s, 3), "X...................", "{rows}x{cols}");
        assert_eq!(s.marks[2], Wrap::Continues, "{rows}x{cols}");
    }
    let s = run(&[b("\x1b[?1049h"), Bytes(full()), Resize(8, 20), b("X")]);
    assert_eq!(
        row(&s, 3),
        "X...................",
        "on the alternate screen"
    );
}

/// **The alternate screen, which is not re-cut**: narrowed past the cursor
/// the wrap stays pending on the new last column, and widened it stays on
/// its own column; either way the `X` wraps to the next row.
#[test]
fn the_alternate_screen_keeps_the_wrap_through_a_width_change() {
    for (cols, kept) in [(15, "abcdefghijklmno"), (10, "abcdefghij")] {
        let s = run(&[b("\x1b[?1049h"), Bytes(full()), Resize(6, cols), b("X")]);
        assert_eq!(row(&s, 2), kept, "narrowed to {cols}");
        assert!(row(&s, 3).starts_with('X'), "narrowed to {cols}");
        assert_eq!(
            s.marks[2],
            Wrap::Continues,
            "narrowed to {cols}: the last column"
        );
    }
    let s = run(&[b("\x1b[?1049h"), Bytes(full()), Resize(6, 30), b("X")]);
    assert_eq!(row(&s, 2), format!("{FULL}.........."));
    assert!(row(&s, 3).starts_with('X'), "widened");
    assert_eq!(s.marks[2], Wrap::Ended, "widened: short of the line's end");
}

/// **The primary, re-cut**: the wrap stays pending on the character it
/// waited after, wherever the re-cut puts it. 20 to 10 ends the text on the
/// new last column and the `X` continues the line; 20 to 15 leaves `t` on
/// column 5 and 20 to 30 on column 20, and the `X` starts the next row.
#[test]
fn the_primary_re_cut_keeps_the_wrap_on_its_character() {
    let s = run(&[Bytes(full()), Resize(6, 10), b("X")]);
    assert_eq!(
        (row(&s, 2), row(&s, 3), row(&s, 4)),
        ("abcdefghij", "klmnopqrst", "X.........")
    );
    assert_eq!((s.marks[2], s.marks[3]), (Wrap::Continues, Wrap::Continues));
    let s = run(&[Bytes(full()), Resize(6, 15), b("X")]);
    assert_eq!(
        (row(&s, 2), row(&s, 3), row(&s, 4)),
        ("abcdefghijklmno", "pqrst..........", "X..............")
    );
    assert_eq!(s.marks[3], Wrap::Ended, "waited on column 5 of 15");
    // **On the character, not one past it**: the report names the `t`.
    let s = run(&[Bytes(full()), Resize(6, 15), b("\x1b[6n")]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[4;5R", WrapPending::Yes)
    );
    let s = run(&[Bytes(full()), Resize(6, 30), b("X")]);
    assert_eq!(row(&s, 2), format!("{FULL}.........."));
    assert!(row(&s, 3).starts_with('X'), "widened");
    assert_eq!(s.marks[2], Wrap::Ended, "waited on column 20 of 30");
    let s = run(&[Bytes(full()), Resize(6, 30), b("\x1b[6n")]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[3;20R", WrapPending::Yes)
    );
}

/// **A save the re-cut carries keeps its pending wrap too**: `ESC 7` and
/// `?1049`'s save of the primary, re-cut 20 to 10, restore a wrap pending
/// on the new last column.
#[test]
fn a_saved_pending_wrap_rides_the_re_cut() {
    for (save, restore) in [("\x1b7\x1b[1;1H", "\x1b8"), ("\x1b[?1049h", "\x1b[?1049l")] {
        let steps = |tail: &str| {
            [
                Bytes(format!("{}{save}", full())),
                Resize(6, 10),
                Bytes(format!("{restore}{tail}")),
            ]
        };
        // On the `t`, the new last column, and not one before it.
        let s = run(&steps("\x1b[6n"));
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[4;10R", WrapPending::Yes),
            "{save:?}"
        );
        let s = run(&steps("X"));
        assert_eq!(
            (row(&s, 3), row(&s, 4)),
            ("klmnopqrst", "X........."),
            "{save:?}"
        );
        assert_eq!(s.marks[3], Wrap::Continues, "{save:?}: the line goes on");
    }
}

// ---- LF, VT, FF, IND, RI, ICH, DCH and a reset of DECAWM -------------------

/// A wrap pending at the fence's right margin, kept by `?69l` on column 10
/// once the margins are the page's again.
fn kept() -> String {
    format!("{}\x1b[?69l", fenced())
}

/// **LF, VT, FF and IND end the wrap on its column**: D.6.1 lists LINE
/// FEED, VERTICAL TAB, FORM FEED and INDEX among the controls that reset
/// the Last Column Flag, and with LNM reset LF makes "no change ... in the
/// Active Column" (LF note 4, p. 5-59), the column the wrap waits on. So
/// the cursor goes down a line on that column and the next character is
/// written there, as xterm's `xtermIndex` (`CursorDown`'s `ResetWrap`,
/// `[[source: xterm-cursor-c]]` 263) and ghostty's `index` do. Before, the
/// wrap went down with the cursor: the `X` wrapped from row 4 to row 5's
/// first column, and the empty row 4 was marked as continuing.
#[test]
fn the_line_feeds_end_the_wrap_on_its_column() {
    for c in ["\n", "\x0b", "\x0c", "\x1bD"] {
        let s = run(&[Bytes(format!("{}{c}\x1b[6n", full()))]);
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[4;20R", WrapPending::No),
            "{c:?}"
        );
        let s = run(&[Bytes(format!("{}{c}X", full()))]);
        assert_eq!(row(&s, 2), FULL, "{c:?}");
        assert_eq!(row(&s, 3), "...................X", "{c:?}");
        assert_eq!(row(&s, 4), "....................", "{c:?}");
        assert_eq!(s.marks[2], Wrap::Ended, "{c:?}: the line ended");
        assert_eq!(s.marks[3], Wrap::Ended, "{c:?}: nothing wrapped");
        // At the fence's right margin and at the wrap `?69l` kept there.
        for (at, pre) in [("fenced", fenced()), ("kept", kept())] {
            let s = run(&[Bytes(format!("{pre}{c}X"))]);
            assert_eq!(row(&s, 2), "....ABCDEF..........", "{c:?} {at}");
            assert_eq!(row(&s, 3), ".........X..........", "{c:?} {at}");
        }
        // On the bottom margin the screen scrolls and the column stays.
        let s = run(&[Bytes(format!("\x1b[6;1H{FULL}{c}X"))]);
        assert_eq!(row(&s, 4), FULL, "{c:?}: scrolled");
        assert_eq!(row(&s, 5), "...................X", "{c:?}: scrolled");
    }
    // Under LNM the line feed's carriage return goes to column 1 first.
    let s = run(&[Bytes(format!("\x1b[20h{}\nX", full()))]);
    assert_eq!(row(&s, 3), "X...................");
}

/// **A VT to a line tabulation stop ends it too**, on the column it waits
/// on of the line the stop is on (`[[source: ecma48]]` 8.3.161, "the
/// corresponding character position").
#[test]
fn a_vt_to_a_line_stop_ends_the_wrap_on_its_column() {
    let s = run(&[Bytes(format!("\x1b[5;1H\x1bJ{}\x0bX", full()))]);
    assert_eq!(row(&s, 3), "....................");
    assert_eq!(row(&s, 4), "...................X");
}

/// **On a double-width line the column the wrap waits on is the half's
/// last**, and LF keeps it onto a single line; onto a double line from a
/// single one it is cut to that line's half, "If the line to which the
/// Active Position is moved contains fewer columns than the Active Column,
/// the Active Column is set to the end of that line" (LF note 3, p. 5-59).
#[test]
fn a_line_feed_from_a_wrap_keeps_the_column_a_line_allows() {
    let s = run(&[b("\x1b[3;1H\x1b#6abcdefghij\nX")]);
    assert_eq!(row(&s, 3), ".........X..........", "off a double line");
    let s = run(&[b("\x1b[4;1H\x1b#6\x1b[3;1Habcdefghijklmnopqrst\nX")]);
    assert!(row(&s, 3).starts_with(".........X"), "{:?}", row(&s, 3));
    assert_eq!(row(&s, 4), "....................", "nothing wrapped");
}

/// **RI ends the wrap on its column**, inside the region and on its top
/// margin, where it scrolls: D.6.1 lists REVERSE_INDEX, and xterm's
/// `RevIndex` calls `CursorUp` and its `ResetWrap` after the scroll too
/// (`[[source: xterm-cursor-c]]` 284, 319-339). ghostty keeps its flag on
/// the top margin (`scrollDown` saves and restores it), pinned in its
/// oracle. Before, the `X` wrapped back onto the full row's `a`, and on
/// the top margin onto the `a` of the line the scroll moved down.
#[test]
fn ri_ends_the_wrap_on_its_column() {
    let s = run(&[Bytes(format!("{}\x1bMX", full()))]);
    assert_eq!(row(&s, 1), "...................X");
    assert_eq!(row(&s, 2), FULL, "the full row untouched");
    for (top, region) in [(1, ""), (2, "\x1b[2;5r")] {
        let s = run(&[Bytes(format!("{region}\x1b[{top};1H{FULL}\x1bMX"))]);
        let y = top - 1;
        assert_eq!(row(&s, y), "...................X", "top margin {top}");
        assert_eq!(row(&s, y + 1), FULL, "top margin {top}: moved down whole");
    }
    let s = run(&[Bytes(format!("{}\x1bMX", kept()))]);
    assert_eq!(row(&s, 1), ".........X..........", "kept");
    assert_eq!(row(&s, 2), "....ABCDEF..........", "kept");
}

/// **ICH and DCH end the wrap and edit the column it waits on**: D.6.1
/// lists INSERT CHARACTER and DELETE_CHARACTER, 070-5's revision items 5
/// and 16 (pp. 5-217, 5-218) add the clearing to both, and each acts "at
/// the Active Position" (pp. 5-142, 5-144), which a pending wrap leaves on
/// that column, as xterm's `InsertChar` and `DeleteChar` reset `do_wrap`
/// (`[[source: xterm-scroll]]` 1924, 2023) and ghostty edits there. Before,
/// both acted one past the line's end, which edited nothing, and the `X`
/// wrapped.
#[test]
fn ich_and_dch_end_the_wrap_and_edit_its_column() {
    for (c, name) in [("\x1b[@", "ICH"), ("\x1b[P", "DCH")] {
        let s = run(&[Bytes(format!("{}{c}\x1b[6n", full()))]);
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[3;20R", WrapPending::No),
            "{name}"
        );
        assert_eq!(row(&s, 2), "abcdefghijklmnopqrs.", "{name}: the t");
        let s = run(&[Bytes(format!("{}{c}X", full()))]);
        assert_eq!(row(&s, 2), "abcdefghijklmnopqrsX", "{name}");
        assert_eq!(row(&s, 3), "....................", "{name}");
        // At the fence the margin's column; at a kept wrap its column.
        let s = run(&[Bytes(format!("{}{c}X", fenced()))]);
        assert_eq!(row(&s, 2), "....ABCDEX..........", "{name} fenced");
        let s = run(&[Bytes(format!("{}{c}X", kept()))]);
        let want = match name {
            "ICH" => "....ABCDEXF.........",
            _ => "....ABCDEX..........",
        };
        assert_eq!(row(&s, 2), want, "{name} kept");
        assert_eq!(row(&s, 3), "....................", "{name} kept");
        // At SLL's line limit, the limit's; DEC's margins, not the limit,
        // bound the shift (`crate::bounds`), so ICH pushes the `e` on.
        let s = run(&[Bytes(format!("\x1b[5 Vabcde{c}X"))]);
        let want = match name {
            "ICH" => "abcdXe..............",
            _ => "abcdX...............",
        };
        assert_eq!(row(&s, 0), want, "{name} at the limit");
    }
}

/// **Right of the side margins they are ignored, and the wrap waits on**:
/// ICH's note 2 and DCH's note 3 (pp. 5-142, 5-144), "this control is
/// ignored", so nothing executes to reset the flag, as xterm's
/// `InsertChar` and `DeleteChar` return before `ResetWrap`
/// (`[[source: xterm-scroll]]` 1898, 1998). ghostty's ICH ends it there,
/// pinned in its oracle.
#[test]
fn ich_and_dch_right_of_the_margins_keep_the_wrap() {
    let edge = format!("{LRM}\x1b[3;15HABCDEF");
    for c in ["\x1b[@", "\x1b[P"] {
        let s = run(&[Bytes(format!("{edge}{c}\x1b[6n"))]);
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[3;20R", WrapPending::Yes),
            "{c:?}"
        );
        let s = run(&[Bytes(format!("{edge}{c}X"))]);
        assert_eq!(row(&s, 2), "..............ABCDEF", "{c:?}");
        assert_eq!(row(&s, 3), "....X...............", "{c:?}");
    }
}

/// **On a row outside the top and bottom margins they act, and end the
/// wrap**: STD 070's margin table gives ICH and DCH "Continues to work
/// outside these margins" for the top and bottom ones (p. 5-24), and a
/// VT420 and a VT520 shifted those rows to the line's end (the `CSI @`
/// row of `docs/vt-coverage.md`). The VT420 manual gives each "no effect
/// outside the scrolling margins" (pp. 152, 153), a conflict taken toward
/// the table and the hardware (a lean). Before 2026-11-01 the wrap stayed
/// and the `X` wrapped to the next row. **With side margins too, a wrap
/// waiting at the screen's last column there ends**: right of the right
/// margin they act on such a row, so the `X` goes on the last column, where
/// until 2026-11-02 the control was ignored and the `X` wrapped.
#[test]
fn ich_and_dch_outside_the_top_and_bottom_margins_end_the_wrap() {
    for (c, name) in [("\x1b[@", "ICH"), ("\x1b[P", "DCH")] {
        let pre = format!("{LRM}\x1b[3;4r\x1b[1;15HABCDEF");
        let s = run(&[Bytes(format!("{pre}{c}\x1b[6n"))]);
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[1;20R", WrapPending::No),
            "{name} right of the margins on row 1"
        );
        let s = run(&[Bytes(format!("{pre}{c}X"))]);
        assert_eq!(row(&s, 0), "..............ABCDEX", "{name}");
        assert_eq!(row(&s, 1), "....................", "{name}");
        // Under HEM's PRECEDING the side margins bound them on every row,
        // so there they are ignored and the wrap waits on.
        let pre = format!("{LRM}\x1b[3;4r\x1b[10h\x1b[1;15HABCDEF");
        let s = run(&[Bytes(format!("{pre}{c}X"))]);
        assert_eq!(row(&s, 0), "..............ABCDEF", "{name} under PRECEDING");
        assert!(
            row(&s, 1).contains('X'),
            "{name} under PRECEDING: X wrapped"
        );
    }
    for (c, name) in [("\x1b[@", "ICH"), ("\x1b[P", "DCH")] {
        // Above the region and below it; on its margins, the controls.
        for y in [1, 5, 2, 4] {
            let pre = format!("\x1b[2;4r\x1b[{y};1H{FULL}");
            let s = run(&[Bytes(format!("{pre}{c}\x1b[6n"))]);
            let cpr = format!("\x1b[{y};20R");
            assert_eq!(
                (s.reply.as_str(), s.pending),
                (cpr.as_str(), WrapPending::No),
                "{name} on row {y}"
            );
            let s = run(&[Bytes(format!("{pre}{c}X"))]);
            assert_eq!(row(&s, y - 1), "abcdefghijklmnopqrsX", "{name} on row {y}");
            assert_eq!(row(&s, y), "....................", "{name} on row {y}");
        }
    }
}

/// **A reset of DECAWM ends the wrap**: D.6.1 lists RESET_MODE
/// (AUTO_WRAP_MODE), and 070-5's revision item 16 (p. 5-217) adds the
/// clearing to Reset Auto Wrap Mode's algorithm, so after `?7l ?7h` the
/// next character overwrites the last column; DECSC saves no wrap and
/// DECCIR reports none. xterm and ghostty keep their flag, pinned in the
/// oracle. A set of DECAWM is not in the list and keeps it. Before, `?7l
/// ?7h` and the `X` wrapped to the next row.
#[test]
fn a_reset_of_decawm_ends_the_wrap() {
    for (at, pre, want) in [
        ("full", full(), "abcdefghijklmnopqrsX"),
        ("fenced", fenced(), "....ABCDEX.........."),
        ("kept", kept(), "....ABCDEX.........."),
    ] {
        let s = run(&[Bytes(format!("{pre}\x1b[?7l\x1b[?7hX"))]);
        assert_eq!(row(&s, 2), want, "{at}");
        assert_eq!(row(&s, 3), "....................", "{at}");
    }
    let s = run(&[Bytes(format!("{}\x1b[?7l\x1b[?7h\x1b[1$w", full()))]);
    assert!(
        s.reply.starts_with("\x1bP1$u3;20;1;@;@;@;"),
        "Sflag without bit 4: {:?}",
        s.reply
    );
    let s = run(&[Bytes(format!("{}\x1b[?7l\x1b7\x1b[?7h\x1b8X", full()))]);
    assert_eq!(row(&s, 2), "abcdefghijklmnopqrsX", "DECSC saved none");
    // XTRESTORE of a reset DECAWM that XTSAVE saved resets it through the
    // same control and ends the wrap alike; of a set one it keeps it.
    for (saved, two, three) in [
        ("l", "abcdefghijklmnopqrsX", "...................."),
        ("h", FULL, "X..................."),
    ] {
        let s = run(&[Bytes(format!(
            "\x1b[?7{saved}\x1b[?7s\x1b[?7h{}\x1b[?7r\x1b[?7hX",
            full()
        ))]);
        let got = (row(&s, 2), row(&s, 3));
        assert_eq!(got, (two, three), "XTRESTORE of ?7{saved}");
    }
    // The control: a set keeps it.
    let s = run(&[Bytes(format!("{}\x1b[?7hX", full()))]);
    assert_eq!(row(&s, 3), "X...................");
}

/// **IRM and a SUB that draws nothing keep the wrap.** D.6.1 names no IRM:
/// its `INSERT_OR_REPLACE_CHARACTER` is the graphic-character procedure
/// (p. 5-139), and IRM's algorithm (p. 5-138) assigns the mode alone, as
/// xterm and ghostty keep their flag. SUB's procedure writes its mark
/// through that one (p. 5-132), which takes the wrap; with the mark off, the
/// default, nothing is written and nothing moves, as ghostty ignores SUB.
#[test]
fn irm_and_an_unmarked_sub_keep_the_wrap() {
    for c in ["\x1b[4h", "\x1b[4l", "\x1a"] {
        let s = run(&[Bytes(format!("{}{c}X", full()))]);
        assert_eq!(row(&s, 2), FULL, "{c:?}");
        assert_eq!(row(&s, 3), "X...................", "{c:?}");
    }
}

// ---- U+FE0F after a kept wrap ----------------------------------------------

/// **A `U+FE0F` widens the character a kept wrap waits after**, in place
/// where the line goes on past it, as it widens one written mid-row: the
/// character takes two columns, the cursor goes past them and the wait
/// ends. A resize between the character and its selector re-cut the line
/// or widened it; a control between them, `?69l` among them, would end the
/// character (`[[source: uax29]]` GB4, GB5), so only a resize reaches here.
/// Before, the selector attached and the character stayed one column wide
/// with the wrap waiting after it.
#[test]
fn u_fe0f_widens_the_character_a_kept_wrap_waits_after() {
    let heart = "\x1b[3;1Habcdefghijklmnopqrs\u{2764}";
    // 20 to 15: the heart on row 4's column 5.
    let s = run(&[b(heart), Resize(6, 15), b("\u{fe0f}\x1b[6n")]);
    assert_eq!(
        (s.reply.as_str(), s.pending),
        ("\x1b[4;7R", WrapPending::No)
    );
    let s = run(&[b(heart), Resize(6, 15), b("\u{fe0f}X")]);
    assert_eq!(row(&s, 3), "pqrs..X........", "re-cut");
    assert_eq!(row(&s, 4), "...............", "nothing wrapped");
    // 20 to 30, on either screen: the heart on column 20.
    for screen in ["", "\x1b[?1049h"] {
        let s = run(&[b(screen), b(heart), Resize(6, 30), b("\u{fe0f}\x1b[6n")]);
        assert_eq!(
            (s.reply.as_str(), s.pending),
            ("\x1b[3;22R", WrapPending::No),
            "widened {screen:?}"
        );
    }
    // The control: written mid-row, the heart widens in place.
    let s = run(&[b("\x1b[3;1Habcd\u{2764}\u{fe0f}X")]);
    assert_eq!(row(&s, 2), "abcd..X.............");
}
