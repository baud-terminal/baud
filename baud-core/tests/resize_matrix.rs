//! Everything a resize must and must not do, in one place.
//!
//! These were written from four things the owner reported over a day of
//! using baud, and each is a behaviour rather than an implementation --
//! so a rule that satisfies one by breaking another fails here rather
//! than in their terminal. That already happened once: not reaching into
//! the history on a growth stopped a drag stacking prompts and made an
//! `ls` disappear when the window came back.
//!
//! `docs/research/notes/m-resize-and-reflow.md` is the design and the
//! measurements; `baud-bench`'s `winch` replays real shell traffic
//! against it.
mod support;
use baud_core::{Term, Zone};
use baud_domain::{PackedCell, Row, WinSize};

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}
fn row_text(t: &Term<'_>, y: usize) -> String {
    text_of(t.row(Row::from_index(y))).trim_end().to_string()
}
fn on_screen(t: &Term<'_>, want: &str) -> bool {
    (0..t.geometry().1.index()).any(|y| row_text(t, y).contains(want))
}
fn count_on_screen(t: &Term<'_>, want: &str) -> usize {
    (0..t.geometry().1.index())
        .filter(|y| row_text(t, *y).contains(want))
        .count()
}
fn resize(t: &mut Term<'_>, cols: usize, rows: usize) {
    let _ = t.resize(&WinSize {
        ws_col: u16::try_from(cols).unwrap(),
        ws_row: u16::try_from(rows).unwrap(),
    });
}

/// A command's output, then the prompt that follows it -- the state a
/// user is in after running `ls`.
fn after_a_command(t: &mut Term<'_>, lines: usize) {
    support::feed(t, b"\x1b]133;C\x07");
    for i in 0..lines {
        support::feed(t, format!("file-{i:02}.txt\r\n").as_bytes());
    }
    support::feed(t, b"\x1b]133;D\x07");
    prompt(t);
}

/// A five-line prompt drawn for the first time.
fn prompt(t: &mut Term<'_>) {
    support::feed(t, b"\x1b]133;P;k=i\x07");
    support::feed(
        t,
        b"zsh prompt\r\n line two\r\n line three\r\n line four\r\n$ ",
    );
    support::feed(t, b"\x1b]133;B\x07");
}

/// The prompt **redrawn**, in the shape the owner's `zsh` actually
/// sends: carriage return, cursor up by the rows it believes its prompt
/// occupies, erase from there to the end of the screen, then a newline
/// and the prompt again.
///
/// Modelling this is the whole point. A helper that merely appends
/// stacks prompts no matter what the terminal does -- a test that cannot
/// pass, which is how the first version of this file read.
fn redraw_prompt(t: &mut Term<'_>) {
    support::feed(t, b"\r\x1b[4A\x1b[J");
    support::feed(t, b"\x1b]133;P;k=i\x07");
    support::feed(
        t,
        b"\r\nzsh prompt\r\n line two\r\n line three\r\n line four\r\n$ ",
    );
    support::feed(t, b"\x1b]133;B\x07");
}

#[test]
fn output_comes_back_when_the_window_does() {
    // The owner: "did ls, then made window smaller then bigger and the
    // ls disappear". A shrink and a growth back to the same size is the
    // identity, whatever the terminal did in between.
    support::with_room(60, |t| {
        after_a_command(t, 12);
        let (cols, rows) = t.geometry();
        assert!(on_screen(t, "file-00"), "the control: the output is there");
        resize(t, cols.index(), 6);
        resize(t, cols.index(), rows.index());
        assert!(
            on_screen(t, "file-00"),
            "the output did not come back when the window did"
        );
    });
}

#[test]
fn a_drag_through_a_short_window_stacks_this_many_prompts() {
    // The owner's own drag, in miniature: down through a window too
    // short to hold the prompt and back up. The shell redraws at every
    // step, and each redraw at a short window scrolls the last one away.
    // Exactly one may be on screen at the end.
    support::with_room(60, |t| {
        after_a_command(t, 8);
        let (cols, rows) = t.geometry();
        for h in [20, 12, 6, 4, 3, 4, 6, 12, 20] {
            resize(t, cols.index(), h);
            // The shell redraws its prompt on every resize, which is
            // what the owner's log shows: two marks a step.
            redraw_prompt(t);
        }
        resize(t, cols.index(), rows.index());
        redraw_prompt(t);
        // **Pinned rather than asserted at one**, for the reason
        // `resize_recorded.rs` spells out: holding the top still took
        // this to one and deleted the owner's `ls` to do it. The bottom
        // anchor reveals the prompts the shell scrolled away while the
        // window was too short to hold one, and a duplicate is a
        // nuisance where a deletion is a loss. Ceiling and floor, so
        // fixing it fails here and says to move the pin.
        assert_eq!(
            count_on_screen(t, "zsh prompt"),
            5,
            "prompts stacked on screen:\n{}",
            (0..t.geometry().1.index())
                .map(|y| format!("{y:3}| {}\n", row_text(t, y)))
                .collect::<String>()
        );
    });
}

#[test]
fn the_top_row_does_not_move_on_a_growth_while_the_shell_owns_the_screen() {
    // The owner, twice: "the current first row changes and it should
    // not". A growth must not slide the screen over the history.
    support::with_room(60, |t| {
        after_a_command(t, 8);
        let (cols, rows) = t.geometry();
        resize(t, cols.index(), rows.index() - 4);
        let top = row_text(t, 0);
        resize(t, cols.index(), rows.index());
        assert_eq!(row_text(t, 0), top, "the top row moved on a growth");
    });
}

#[test]
fn a_shrink_and_a_growth_are_the_identity_for_the_whole_screen() {
    // Stronger than the two above: every row comes back where it was.
    support::with_room(60, |t| {
        after_a_command(t, 10);
        let (cols, rows) = t.geometry();
        let before: Vec<String> = (0..rows.index()).map(|y| row_text(t, y)).collect();
        resize(t, cols.index(), 8);
        resize(t, cols.index(), rows.index());
        let after: Vec<String> = (0..rows.index()).map(|y| row_text(t, y)).collect();
        assert_eq!(
            before, after,
            "a shrink and a growth back is not the identity"
        );
    });
}

#[test]
fn the_zone_is_what_these_rules_key_on() {
    // The control for the whole file: without the marks every test here
    // would be about a different code path, so the state they establish
    // is asserted rather than assumed.
    support::with_room(60, |t| {
        after_a_command(t, 4);
        assert_eq!(t.zone(), Zone::Prompt);
        assert!(t.semantic_marks >= 4, "the marks were seen");
    });
}

#[test]
fn a_shrink_does_not_let_the_prompt_overwrite_output() {
    // **The owner, and the worst report yet:** "change size to small and
    // big actually DELETE the ls and it never comes back".
    //
    // What `output_comes_back_when_the_window_does` missed is that the
    // shell **redraws while shrunk**. If a shrink moves the cursor to a
    // row that is not the one it was on, the shell's next redraw lands
    // there -- over whatever output is at that row -- and no later
    // growth can bring back cells that were overwritten rather than
    // hidden. A test that shrinks and grows without letting the shell
    // speak in between cannot see it.
    support::with_room(60, |t| {
        after_a_command(t, 12);
        let (cols, rows) = t.geometry();
        assert!(on_screen(t, "file-00"), "the control: the output is there");

        // Small, and the shell redraws its prompt at whatever row the
        // cursor now sits on -- which is the whole point.
        resize(t, cols.index(), 8);
        redraw_prompt(t);
        // And back.
        resize(t, cols.index(), rows.index());
        redraw_prompt(t);

        let missing: Vec<String> = (0..12)
            .map(|i| format!("file-{i:02}.txt"))
            .filter(|l| !on_screen(t, l))
            .collect();
        assert!(
            missing.is_empty(),
            "the prompt overwrote output that no growth can restore: {missing:?}\n{}",
            (0..t.geometry().1.index())
                .map(|y| format!("{y:3}| {}\n", row_text(t, y)))
                .collect::<String>()
        );
    });
}
