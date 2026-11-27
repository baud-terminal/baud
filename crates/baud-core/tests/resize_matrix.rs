//! Everything a resize must and must not do, in one place.
//!
//! These were written from four things a user reported over a day of
//! using baud, and each is a behaviour rather than an implementation --
//! so a rule that satisfies one by breaking another fails here rather
//! than in their terminal. That already happened once: not reaching into
//! the history on a growth stopped a drag stacking prompts and made an
//! `ls` disappear when the window came back.
//!
//! `baud-bench`'s `winch` replays real shell traffic against it.
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
    text_of(t.row(Row::from_index(y)).expect("a row on the grid"))
        .trim_end()
        .to_string()
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
    // Enter: the input line ends before the output starts.
    support::feed(t, b"\r\n\x1b]133;C\x07");
    for i in 0..lines {
        support::feed(t, format!("file-{i:02}.txt\r\n").as_bytes());
    }
    support::feed(t, b"\x1b]133;D\x07");
    prompt(t);
}

/// zsh, as the fixtures record it: the mark, then a newline that is
/// part of the prompt, then four lines with the cursor on the last -- a
/// block of five rows whose first, the mark's, is blank. The redraw
/// moves up four from the cursor to that row. An earlier version of
/// this model sent the newline on the redraw and not on the first
/// prompt, so its `CUU 4` landed one row low and every redraw drifted
/// the block down a row: a model wrong in a way the recordings are not.
fn prompt(t: &mut Term<'_>) {
    support::feed(t, b"\x1b]133;P;k=i\x07");
    support::feed(t, b"\r\nzsh prompt\r\n line two\r\n line three\r\n$ ");
    support::feed(t, b"\x1b]133;B\x07");
}

/// The prompt **redrawn**, in the shape a real `zsh` sends: carriage
/// return, cursor up by the rows it believes its prompt occupies, erase
/// from there to the end of the screen, then a newline and the prompt
/// again.
///
/// Modelling this is the whole point. A helper that merely appends
/// stacks prompts no matter what the terminal does -- a test that cannot
/// pass, which is how the first version of this file read.
fn redraw_prompt(t: &mut Term<'_>) {
    support::feed(t, b"\r\x1b[4A\x1b[J");
    prompt(t);
}

#[test]
fn output_comes_back_when_the_window_does() {
    // A user's report: "did ls, then made window smaller then bigger
    // and the ls disappear". A shrink and a growth back to the same
    // size is the identity, whatever the terminal did in between.
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
    // A recorded drag, in miniature: down through a window too short to
    // hold the prompt and back up. The shell redraws at every step, and
    // each redraw at a short window scrolls the last one away. Exactly
    // one may be on screen at the end.
    support::with_room(60, |t| {
        after_a_command(t, 8);
        let (cols, rows) = t.geometry();
        for h in [20, 12, 6, 4, 3, 4, 6, 12, 20] {
            resize(t, cols.index(), h);
            // The shell redraws its prompt on every resize, which is
            // what a recorded log shows: two marks a step.
            redraw_prompt(t);
        }
        resize(t, cols.index(), rows.index());
        redraw_prompt(t);
        // **One.** This sat pinned at five while the bottom anchor
        // revealed the prompts the shell scrolled away in a window too
        // short to hold one -- a duplicate being a nuisance where the
        // top anchor's deletion was a loss. The redraw mark's erase
        // reaches through the history to the block's recorded first
        // row and forgets those rows, so the growth blanks instead of
        // pulling them back; `a_block_taller_than_the_screen_is_erased_
        // through_the_history_on_a_redraw` is the mechanism alone.
        assert_eq!(
            count_on_screen(t, "zsh prompt"),
            1,
            "prompts stacked on screen:\n{}",
            (0..t.geometry().1.index())
                .map(|y| format!("{y:3}| {}\n", row_text(t, y)))
                .collect::<String>()
        );
    });
}

#[test]
fn the_top_row_does_not_move_on_a_growth_while_the_shell_owns_the_screen() {
    // A user's report, twice: "the current first row changes and it
    // should not". A growth must not slide the screen over the
    // history.
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
    // **The worst report yet:** "change size to small and big actually
    // DELETE the ls and it never comes back".
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

/// A fresh terminal: nothing before the prompt.
fn fresh(t: &mut Term<'_>) {
    prompt(t);
}

/// The whole screen, trimmed rows.
fn screen(t: &Term<'_>) -> Vec<String> {
    (0..t.geometry().1.index())
        .map(|y| row_text(t, y))
        .collect()
}

#[test]
fn a_fresh_terminal_dragged_through_a_tiny_window_comes_back_as_it_was() {
    // A recording of 2026-09-18 02:18: a fresh terminal, the prompt at
    // the top, the window dragged small and back -- and the prompt sits
    // at the bottom of the same-sized window with a screen of blank
    // rows above it. Every row must read as it did, and the history
    // must hold nothing, because nothing exceeded the screen at this
    // size.
    support::with_room(60, |t| {
        resize(t, 100, 28);
        fresh(t);
        let want = screen(t);
        assert_eq!(
            &want[..2],
            ["", "zsh prompt"],
            "the control: the prompt is at the top"
        );
        for h in [20, 12, 6, 4, 3, 2, 3, 4, 6, 12, 20, 28] {
            resize(t, 100, h);
            redraw_prompt(t);
        }
        assert_eq!(
            t.scrollback().len(),
            0,
            "a fresh terminal has nothing to retain, and holds {} lines",
            t.scrollback().len()
        );
        assert_eq!(screen(t), want, "the screen is not what it was");
    });
}

#[test]
fn a_fresh_terminal_dragged_through_a_tiny_window_and_back_by_bare_resizes() {
    // The same, with the shell silent for the way back: the growth alone
    // must not pull manufactured rows above the prompt.
    support::with_room(60, |t| {
        resize(t, 100, 28);
        fresh(t);
        let want = screen(t);
        for h in [12, 4, 2] {
            resize(t, 100, h);
            redraw_prompt(t);
        }
        for h in [4, 12, 28] {
            resize(t, 100, h);
        }
        assert_eq!(t.scrollback().len(), 0);
        assert_eq!(
            screen(t)[..6],
            want[..6],
            "the prompt is not where it was after a silent growth"
        );
    });
}

#[test]
fn output_and_the_prompt_stay_where_they_were_through_a_drag() {
    // The recording's second half: an `ls`, then the drag. The output
    // and the prompt below it come back exactly, with no blank rows
    // above the output that were not there.
    support::with_room(60, |t| {
        resize(t, 100, 28);
        fresh(t);
        after_a_command(t, 8);
        let want = screen(t);
        assert_eq!(want[1], "zsh prompt");
        assert!(
            want.iter().any(|l| l == "file-00.txt"),
            "the control: the output is there"
        );
        for h in [20, 12, 6, 3, 2, 3, 6, 12, 20, 28] {
            resize(t, 100, h);
            redraw_prompt(t);
        }
        assert_eq!(screen(t), want, "the screen is not what it was");
    });
}

#[test]
fn a_shell_that_sends_no_marks_is_left_exactly_as_before() {
    // The control for every rule keyed on the marks: without them the
    // terminal knows nothing about prompts and does what it did.
    support::with_room(60, |t| {
        resize(t, 100, 28);
        support::feed(t, b"plain prompt\r\n$ ");
        assert_eq!(t.zone(), Zone::Output);
        for h in [12, 4, 2, 4, 12, 28] {
            resize(t, 100, h);
            support::feed(t, b"\r\x1b[1A\x1b[J\r\nplain prompt\r\n$ ");
        }
        assert_eq!(t.prompt_erases, 0, "no mark, no erase");
        assert_eq!(count_on_screen(t, "plain prompt"), 1);
    });
}

/// The prompt as the shell draws it for a screen `width` wide: the first
/// line padded to that width, which is what a zsh with a right prompt
/// does. Drawn on a narrower screen it wraps, and the block is a row
/// taller than the shell believes.
fn prompt_for_width(t: &mut Term<'_>, width: usize) {
    support::feed(t, b"\x1b]133;P;k=i\x07\r\n");
    let mut first = b"zsh prompt".to_vec();
    first.resize(width, b' ');
    support::feed(t, &first);
    support::feed(t, b"\r\n line two\r\n line three\r\n$ ");
    support::feed(t, b"\x1b]133;B\x07");
}

fn stale_redraw(t: &mut Term<'_>, width: usize) {
    support::feed(t, b"\r\x1b[4A\x1b[J");
    prompt_for_width(t, width);
}

#[test]
fn a_redraw_computed_for_a_wider_screen_does_not_move_the_block_down() {
    // A burst: the screen narrows twice before the shell's first redraw
    // arrives, so that redraw is padded to the wider width, wraps, and its
    // `CUU 4` counted a block one row shorter than it drew. Then the
    // redraw for the real width. The block is where it was.
    support::with_room(60, |t| {
        resize(t, 100, 28);
        support::feed(t, b"out\r\n");
        prompt_for_width(t, 100);
        let want = screen(t);
        assert_eq!(want[0], "out");
        assert_eq!(want[2], "zsh prompt");
        resize(t, 80, 28);
        resize(t, 60, 28);
        stale_redraw(t, 100);
        stale_redraw(t, 80);
        stale_redraw(t, 60);
        assert_eq!(t.prompt_erases, 3);
        assert_eq!(screen(t), want, "the block moved");
        assert_eq!(t.scrollback().len(), 0);
    });
}

/// A line of `n` characters that says which line it is at both ends, so a
/// tail cut at the edge is visible as a missing string.
fn long_name(i: usize, n: usize) -> String {
    let body: String = (0..n - 12)
        .map(|k| char::from(b'a' + u8::try_from(k % 26).unwrap_or(0)))
        .collect();
    format!("head{i:02}-{body}-tail{i:02}")
}

#[test]
fn a_program_that_forwards_marks_does_not_decide_the_shells_reflow() {
    // A zsh runs a full-screen program, the window is dragged narrow while
    // it runs, and it exits. The command output above the prompt is re-cut
    // to the narrow width -- every line's tail on a row of its own --
    // whether or not the program forwarded an inner shell's marks on its
    // own screen. It was not when it did: the resize read the program's
    // zone for the primary's reflow and left the output cut at the edge.
    // The arm with no mark is the control and runs first.
    for forwards in [false, true] {
        support::with_room(60, |t| {
            support::feed(t, b"\r\n\x1b]133;C\x07");
            for i in 0..4 {
                support::feed(t, format!("{}\r\n", long_name(i, 70)).as_bytes());
            }
            support::feed(t, b"\x1b]133;D\x07");
            prompt(t);
            let rows = t.geometry().1.index();
            // The user runs the program.
            support::feed(t, b"\r\n\x1b]133;C\x07\x1b[?1049h");
            if forwards {
                support::feed(t, b"\x1b]133;P;k=i\x07inner $ \x1b]133;B\x07");
            }
            resize(t, 40, rows);
            support::feed(t, b"\x1b[?1049l\x1b]133;D\x07");
            prompt(t);
            for i in 0..4 {
                let name = long_name(i, 70);
                assert!(
                    on_screen(t, &name[..40]) && on_screen(t, &name[40..]),
                    "forwards {forwards}: line {i} was not re-cut to the narrow width:\n{:#?}",
                    screen(t)
                );
            }
            assert_eq!(t.zone(), Zone::Prompt, "forwards {forwards}");
            assert_eq!(
                t.prompt_erases, 0,
                "forwards {forwards}: nothing was redrawn"
            );
        });
    }
}

#[test]
fn a_reset_leaves_no_history_for_a_resize_to_bring_back() {
    // A zsh runs `reset` after a long listing, and the window is then
    // made shorter and taller again. The reset's bytes are the real ones
    // (`tools/tests/reset/`); the prompt it leaves stays at the top, and no
    // line from before it comes back -- the history is gone, and a growth
    // has nothing to pull. It pulled the reset's stale history count: the
    // prompt slid down twenty-seven rows over blanks.
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tools/tests/reset/seq-reset.raw"
    ))
    .expect("the capture");
    let at = raw
        .windows(2)
        .position(|w| w == b"\x1bc")
        .expect("the capture's ESC c");
    // From the tab-stop reset before `ESC c` to the output after the modes.
    let start = raw[..at]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let end = at + raw[at..].iter().position(|&b| b == b'>').expect("ESC >") + 1;
    let reset = &raw[start..end];
    support::with_room(60, |t| {
        after_a_command(t, 70);
        assert!(
            !t.scrollback().is_empty(),
            "the control: the listing scrolled"
        );
        support::feed(t, b"\r\n\x1b]133;C\x07");
        support::feed(t, reset);
        support::feed(t, b"\x1b]133;D\x07");
        prompt(t);
        let (cols, rows) = t.geometry();
        let want = screen(t);
        assert_eq!(want[1], "zsh prompt", "the prompt is at the top");
        resize(t, cols.index(), 10);
        resize(t, cols.index(), rows.index());
        assert_eq!(screen(t), want, "the screen moved over nothing");
        assert_eq!(t.scrollback().len(), 0, "the reset cleared the history");
        assert!(
            !screen(t).iter().any(|l| l.starts_with("file-")),
            "a line from before the reset came back"
        );
    });
}
