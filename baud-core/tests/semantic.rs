//! `OSC 133` marks the boundary a resize must not re-cut across.
//!
//! A shell redraws its prompt after a resize by counting the rows it
//! occupies, moving the cursor up by that many and erasing from there.
//! Re-cutting those rows to a new width changes the count behind its
//! back, so the erase begins below the prompt's first line and that line
//! survives -- once per resize. Output has no such reader.
//!
//! See `docs/research/notes/m-resize-and-reflow.md` for the measurement
//! and for what the alternatives cost.
mod support;
use baud_core::{PackedCell, Resized, Term, Zone};
use baud_domain::{Back, Row, WinSize};

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

/// A line longer than the narrow width, so a re-cut is visible as a
/// second row rather than as nothing.
fn long_line(n: usize) -> String {
    //  rather than a cast: the audit counts casts, and a
    // budget is not moved to fit a test either.
    (0..n)
        .map(|i| char::from(b'a' + u8::try_from(i % 26).unwrap_or(0)))
        .collect()
}

#[test]
fn a_prompt_is_not_recut_by_a_resize() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            // Output first, then the prompt mark, then a line wider than
            // the width we will narrow to.
            support::feed(t, b"output line\r\n");
            support::feed(t, b"\x1b]133;P;k=i\x07");
            support::feed(t, long_line(36).as_bytes());
            assert_eq!(t.semantic_marks, 1, "the mark was seen");
            assert_eq!(t.zone(), Zone::Prompt, "the mark opened the prompt zone");
            let before = row_text(t, 1);
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 8,
            });
            // The prompt's row keeps its layout: the shell will redraw it
            // and its own count is what must stay valid.
            assert_eq!(
                row_text(t, 1)[..20.min(before.len())],
                before[..20.min(before.len())],
                "the prompt row was re-cut"
            );
            assert!(
                row_text(t, 2).is_empty(),
                "the prompt was wrapped onto a second row: {:?}",
                row_text(t, 2)
            );
        },
    );
}

#[test]
fn output_is_still_recut_by_a_resize() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            // The same line, after the mark that says the command's output
            // has begun: nothing will redraw it, so re-wrapping it is the
            // half of a reflow worth keeping.
            support::feed(t, b"\x1b]133;P;k=i\x07");
            support::feed(t, b"\x1b]133;C\x07");
            support::feed(t, long_line(36).as_bytes());
            assert_eq!(t.semantic_marks, 2);
            assert_eq!(t.zone(), Zone::Output, "the output mark closed the zone");
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 8,
            });
            assert!(
                !row_text(t, 1).is_empty(),
                "output was not re-cut: row 1 is empty, so the tail was dropped rather than wrapped"
            );
        },
    );
}

#[test]
fn a_stream_with_no_marks_is_recut_as_it_always_was() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            support::feed(t, long_line(36).as_bytes());
            assert_eq!(t.semantic_marks, 0, "the control: no marks were sent");
            assert_eq!(
                t.zone(),
                Zone::Output,
                "a terminal that has seen no marks is in the output zone"
            );
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 8,
            });
            assert!(
                !row_text(t, 1).is_empty(),
                "a stream with no shell integration must behave as before"
            );
        },
    );
}

#[test]
fn the_prompt_zone_opens_at_a_mark_and_closes_at_the_output() {
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            // `A` and `P` both open it, `B` keeps it -- the user typing is
            // still the shell's region -- and `C` closes it.
            for open in [&b"\x1b]133;A\x07"[..], &b"\x1b]133;P;k=i\x07"[..]] {
                support::feed(t, b"\x1b]133;C\x07");
                support::feed(t, long_line(36).as_bytes());
                let _ = t.resize(&WinSize {
                    ws_col: 20,
                    ws_row: 8,
                });
                assert!(!row_text(t, 1).is_empty(), "output re-cuts");
                let _ = t.resize(&WinSize {
                    ws_col: 40,
                    ws_row: 8,
                });

                support::feed(t, b"\x1b[2J\x1b[H");
                support::feed(t, open);
                support::feed(t, b"\x1b]133;B\x07");
                support::feed(t, long_line(36).as_bytes());
                let _ = t.resize(&WinSize {
                    ws_col: 20,
                    ws_row: 8,
                });
                assert!(
                    row_text(t, 1).is_empty(),
                    "the prompt zone re-cut after {open:?}"
                );
                support::feed(t, b"\x1b[2J\x1b[H");
                let _ = t.resize(&WinSize {
                    ws_col: 40,
                    ws_row: 8,
                });
            }
        },
    );
}

#[test]
fn a_taller_screen_reaches_into_the_history_and_that_is_the_anchor() {
    // **This pinned the opposite once**, and the reversal is the design
    // rather than a regression. Refusing to reach into the history on a
    // growth stopped a drag revealing prompts the shell had scrolled
    // away -- and it left the rows a shrink retired behind, which is the
    // owner's `ls` deleted. The anchor is the bottom: the window sits at
    // the end of the content, so a taller one shows more of it above,
    // and what a shrink pushed up comes back down.
    support::with_room(60, |t| {
        support::feed(t, b"\x1b]133;C\x07");
        for i in 0..40 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        let (cols, rows) = t.geometry();
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: 6,
        });
        let retired = t.scrollback().len();
        assert!(
            retired > 6,
            "the control: the shrink retired rows, so a growth has something to reach for"
        );
        support::feed(t, b"\x1b]133;D\x07\x1b]133;P;k=i\x07$ ");
        assert_eq!(t.zone(), Zone::Prompt);
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: u16::try_from(rows.index()).unwrap(),
        });
        assert!(
            t.scrollback().len() < retired,
            "the growth left the retired rows in the history: {} of {retired}",
            t.scrollback().len()
        );
    });
}

#[test]
fn the_prompt_row_is_where_the_mark_was_and_follows_the_screen() {
    // The block start is a ring row, so it has to follow its cells through
    // a scroll, a shrink's retirement and a growth's pull -- and the
    // distance from it to the cursor is what a resize must not change.
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            assert_eq!(t.prompt_row(), None, "no mark yet, so no block");
            support::feed(t, b"one\r\ntwo\r\nthree\r\n");
            support::feed(t, b"\x1b]133;P;k=i\x07prompt$ ");
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(3)),
                "the block starts where the cursor was at the mark"
            );
            // Four rows down inside the block: to the bottom, no scroll.
            support::feed(t, b"\r\n\r\n\r\n\r\n");
            assert_eq!(t.cursor().0, Row::from_index(7));
            assert_eq!(t.prompt_row(), Some(Row::from_index(3)));
            // A shorter screen retires two rows off the top: the cursor and
            // the block start both move up two, and the distance holds.
            let _ = t.resize(&WinSize {
                ws_col: 40,
                ws_row: 6,
            });
            assert_eq!(t.cursor().0, Row::from_index(5));
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(1)),
                "the block start moved with its cells, not with the screen edge"
            );
            // Two scrolls push the block's first row off the top.
            support::feed(t, b"\r\n\r\n");
            assert_eq!(
                t.prompt_row(),
                None,
                "a first row that has left the screen is not a screen row"
            );
            // The shell redraws: up two, mark again -- a new start at the
            // cursor. The block's stale first row, one behind the window,
            // is the redraw's to replace: the window moves back over it
            // and it is erased, so the history holds the three output
            // lines and nothing the terminal made.
            support::feed(t, b"\r\x1b[2A\x1b]133;P;k=i\x07");
            assert_eq!(
                t.cursor().0,
                Row::from_index(0),
                "on the block's first row, now row 0"
            );
            assert_eq!(t.prompt_row(), Some(Row::from_index(0)));
            assert_eq!(t.prompt_erases, 1);
            assert_eq!(
                t.scrollback().len(),
                3,
                "one, two, three -- and no stale prompt row"
            );
            // A taller screen pulls two retired rows back -- content first,
            // before the blank row the window gave up -- so both move down two.
            let _ = t.resize(&WinSize {
                ws_col: 40,
                ws_row: 8,
            });
            assert_eq!(t.cursor().0, Row::from_index(2));
            assert_eq!(t.prompt_row(), Some(Row::from_index(2)));
            assert_eq!(t.scrollback().len(), 1);
            // And the output mark closes the block.
            support::feed(t, b"\x1b]133;C\x07");
            assert_eq!(t.prompt_row(), None, "output has no prompt row");
        },
    );
}

#[test]
fn a_continuation_mark_keeps_the_block_start_where_the_initial_one_put_it() {
    // kitty's zsh integration marks the PS2 line with `A;k=s` and fish
    // marks its continuation lines; a right prompt is `k=r` on the same
    // row. None of them begins a new block, so none may move the start --
    // and a bare `A` is the control that a fresh mark still does.
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
        |t| {
            support::feed(t, b"one\r\ntwo\r\nthree\r\n");
            support::feed(t, b"\x1b]133;A;k=i;cl=line\x07line one\r\n");
            assert_eq!(t.prompt_row(), Some(Row::from_index(3)));
            support::feed(t, b"\x1b]133;A;k=s\x07line two\r\n");
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(3)),
                "k=s is the same block"
            );
            support::feed(t, b"\x1b]133;A;k=c\x07line three");
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(3)),
                "k=c is the same block"
            );
            support::feed(t, b"\x1b]133;A;k=r\x07 right");
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(3)),
                "k=r is the same block"
            );
            assert_eq!(
                t.zone(),
                Zone::Prompt,
                "every kind keeps the prompt zone open"
            );
            // The control: after `D` -- the way a fresh prompt arrives -- a
            // bare initial mark starts a new block here. (In the prompt
            // zone it would be a redraw, and the cursor would go back to
            // the block's first row.)
            support::feed(t, b"\r\n\x1b]133;D\x07\x1b]133;A\x07next");
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(6)),
                "a bare A is an initial line and moves the start"
            );
        },
    );
}

#[test]
fn a_redraw_mark_erases_the_block_it_replaces_and_nothing_above_it() {
    // The shell's `CUU n` undershoots -- two rows where the block is
    // three -- so its own erase misses the block's first row. The redraw
    // mark says what is being replaced, and the terminal erases exactly
    // the recorded block: the stale first row goes, the output above it
    // stays, and the counter says it happened.
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 10,
        },
        |t| {
            support::feed(t, b"output one\r\noutput two\r\n");
            support::feed(
                t,
                b"\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ",
            );
            assert_eq!(t.prompt_row(), Some(Row::from_index(2)));
            assert_eq!(t.cursor().0, Row::from_index(5));
            // Up two, not three; erase; mark; redraw.
            support::feed(t, b"\r\x1b[2A\x1b[J\x1b]133;P;k=i\x07");
            assert_eq!(t.prompt_erases, 1, "the redraw mark erased the block");
            assert_eq!(
                row_text(t, 0),
                "output one",
                "output above the block is untouched"
            );
            assert_eq!(row_text(t, 1), "output two");
            assert_eq!(row_text(t, 2), "", "the block's stale first row is gone");
            // The mark puts the cursor on the block's first row, two,
            // where the shell's count fell short.
            assert_eq!(
                t.cursor().0,
                Row::from_index(2),
                "the cursor is on the block's first row"
            );
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(2)),
                "and the block starts there"
            );
            support::feed(t, b"line one\r\nline two\r\nline three\r\n$ ");
            let lines: Vec<String> = (0..10).map(|y| row_text(t, y)).collect();
            assert_eq!(
                lines.iter().filter(|l| l.as_str() == "line one").count(),
                1,
                "one copy of the prompt:\n{lines:#?}"
            );
            // The control: a first mark after output erases nothing.
            support::feed(t, b"\x1b]133;C\x07done\r\n\x1b]133;P;k=i\x07$ ");
            assert_eq!(
                t.prompt_erases, 1,
                "a fresh prompt after output is not a redraw"
            );
        },
    );
}

#[test]
fn a_block_taller_than_the_screen_is_taken_back_from_the_history_on_a_redraw() {
    // `drag`, step 146: a window too short for the prompt. The shell's
    // `CUU n` stops at row 0, its erase clears the screen, and the block's
    // first row -- scrolled into the history by the block's own height --
    // is what a later growth pulls back beside the redrawn one. At the
    // redraw mark the window moves back over that row -- it is the
    // block's -- and erases it with the rest of the block, so the history
    // holds only the output and a growth pulls back output.
    //
    // Provisioned at the tall size and filled first. Three drafts of this
    // test read "zero copies" off a screen that had never grown: one sized
    // to the short window, whose growth was `Truncated`; two built by
    // `with_room` at the corpus's fifty rows on an eight-row storage, whose
    // first resize *hid* forty-two rows that the growth then revealed
    // instead of pulling. A growth pulls only when nothing is hidden, so the
    // shrink has to retire -- the cursor at the bottom -- and the storage
    // has to be the tall screen exactly.
    let tall = WinSize {
        ws_col: 40,
        ws_row: 8,
    };
    support::with_term(&tall, |t| {
        for _ in 0..7 {
            support::feed(t, b"out\r\n");
        }
        assert_eq!(t.cursor().0, Row::from_index(7));
        let short = WinSize {
            ws_col: 40,
            ws_row: 3,
        };
        assert_eq!(t.resize(&short), Resized::Ok);
        assert_eq!(
            t.scrollback().len(),
            5,
            "the shrink retired five rows of output"
        );
        support::feed(
            t,
            b"\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ",
        );
        assert_eq!(t.scrollback().len(), 8, "the block scrolled three more off");
        assert_eq!(t.prompt_row(), None, "its first row is not a screen row");
        // The shell's redraw: up three, which stops at row 0; erase; mark.
        support::feed(t, b"\r\x1b[3A\x1b[J\x1b]133;P;k=i\x07");
        assert_eq!(t.prompt_erases, 1, "the redraw mark erased the block");
        assert_eq!(
            t.scrollback().len(),
            7,
            "the row behind the window was the block's: the window took it back"
        );
        assert_eq!(
            t.hidden().index(),
            1,
            "and gave up one blank row below, hidden"
        );
        assert_eq!(
            t.scrollback()
                .line(Back::NEWEST)
                .map(|(c, _)| text_of(c).trim_end().to_string()),
            Some("out".to_string()),
            "the newest retained line is output, not a prompt row"
        );
        support::feed(t, b"line one\r\nline two\r\nline three\r\n$ ");
        assert_eq!(t.resize(&tall), Resized::Ok);
        let lines: Vec<String> = (0..8).map(|y| row_text(t, y)).collect();
        assert_eq!(
            lines.iter().filter(|l| l.as_str() == "line one").count(),
            1,
            "one copy after the growth:\n{lines:#?}"
        );
        assert_eq!(
            lines,
            [
                "out",
                "out",
                "out",
                "out",
                "line one",
                "line two",
                "line three",
                "$"
            ],
            "the growth pulls the retained rows back in their order: four rows of output \
             untouched, then the redrawn block -- and no blank row where a stale one was"
        );
    });
}

#[test]
fn a_redraw_of_a_block_taller_than_the_screen_takes_its_rows_back_from_the_history() {
    // The mechanism the fresh-terminal tests need. A block taller than the
    // window scrolls its top rows off; at the redraw mark those rows are
    // the block's and nothing else's, so the window moves back up over
    // them -- the block's first row becomes row 0, where the shell's
    // clamped `CUU n` left the cursor -- rather than leaving them behind
    // as history for a growth to pull back above the prompt. What the
    // window gives up at the bottom is the blank the shell's own `ED`
    // just made, and it is hidden, which is what a growth reveals first.
    let tall = WinSize {
        ws_col: 40,
        ws_row: 8,
    };
    support::with_term(&tall, |t| {
        support::feed(
            t,
            b"\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ",
        );
        let want: Vec<String> = (0..8).map(|y| row_text(t, y)).collect();
        assert_eq!(&want[..4], ["line one", "line two", "line three", "$"]);
        assert_eq!(
            t.resize(&WinSize {
                ws_col: 40,
                ws_row: 2,
            }),
            Resized::Ok
        );
        // Two rows: the shell redraws four lines, scrolling two off each time.
        for _ in 0..3 {
            support::feed(
                t,
                b"\r\x1b[3A\x1b[J\x1b]133;P;k=i\x07line one\r\nline two\r\nline three\r\n$ ",
            );
        }
        assert_eq!(t.prompt_erases, 3);
        assert_eq!(t.resize(&tall), Resized::Ok);
        let got: Vec<String> = (0..8).map(|y| row_text(t, y)).collect();
        assert_eq!(
            got, want,
            "the block is where it was, with nothing above it"
        );
        assert_eq!(
            t.scrollback().len(),
            0,
            "nothing was manufactured into the history"
        );
    });
}

#[test]
fn an_enter_at_the_prompt_leaves_the_old_prompt_where_it_was() {
    // The owner, 2026-09-18: "when pressed enter prompt disappear for a
    // few lines". Enter on an empty line is `B` (input), the newline,
    // `D` (done) and a new `P` -- no `C`, because nothing ran. That `P`
    // is a *fresh* prompt, not a redraw of the one above it, and nothing
    // may be erased: both prompts stay, in place.
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 12,
        },
        |t| {
            support::feed(
                t,
                b"\x1b]133;P;k=i\x07line one\r\nline two\r\n$ \x1b]133;B\x07",
            );
            // Enter: the shell moves to a new line, reports done, prompts.
            support::feed(
                t,
                b"\r\n\x1b]133;D;0\x07\x1b]133;P;k=i\x07line one\r\nline two\r\n$ \x1b]133;B\x07",
            );
            let lines: Vec<String> = (0..12).map(|y| row_text(t, y)).collect();
            assert_eq!(t.prompt_erases, 0, "a prompt after Enter is not a redraw");
            assert_eq!(
                &lines[..6],
                ["line one", "line two", "$", "line one", "line two", "$"],
                "both prompts, in place:\n{lines:#?}"
            );
            assert_eq!(
                t.prompt_row(),
                Some(Row::from_index(3)),
                "the new block starts where it was drawn"
            );
            // And a redraw of the *new* prompt still erases only the new one.
            support::feed(
                t,
                b"\r\x1b[2A\x1b[J\x1b]133;P;k=i\x07line one\r\nline two\r\n$ ",
            );
            assert_eq!(t.prompt_erases, 1);
            let lines: Vec<String> = (0..12).map(|y| row_text(t, y)).collect();
            assert_eq!(
                &lines[..6],
                ["line one", "line two", "$", "line one", "line two", "$"]
            );
        },
    );
}

#[test]
fn a_redraw_mark_puts_the_cursor_on_the_block_it_replaces() {
    // The shell's `CUU n` counts the rows it believes its prompt has; a
    // redraw computed for a wider screen counts one fewer than the
    // wrapped block on this one, and lands below the block's first row.
    // The mark says "the prompt starts here", and the terminal knows
    // where the block starts: the cursor is put there, so the redraw
    // lands on the block it replaces and nothing drifts down.
    support::with_term(
        &WinSize {
            ws_col: 40,
            ws_row: 12,
        },
        |t| {
            support::feed(
                t,
                b"above\r\n\x1b]133;P;k=i\x07\r\nline one\r\nline two\r\n$ ",
            );
            assert_eq!(t.prompt_row(), Some(Row::from_index(1)));
            assert_eq!(t.cursor().0, Row::from_index(4));
            // Up two where the block is three: the shell undershoots.
            support::feed(t, b"\r\x1b[2A\x1b[J\x1b]133;P;k=i\x07");
            assert_eq!(
                t.cursor().0,
                Row::from_index(1),
                "the cursor is on the block's first row"
            );
            assert_eq!(t.prompt_row(), Some(Row::from_index(1)));
            support::feed(t, b"\r\nline one\r\nline two\r\n$ ");
            let lines: Vec<String> = (0..7).map(|y| row_text(t, y)).collect();
            assert_eq!(
                lines,
                ["above", "", "line one", "line two", "$", "", ""],
                "the block is redrawn where it was, with nothing above it touched"
            );
        },
    );
}

// **Each screen has its own prompt state.** The zone and the block start
// were one field on `Cold` while the kitty stacks beside them were swapped
// per screen, so a mark on one screen was read against the other's ring:
// a redraw on the alternate screen erased its rows by the primary's block
// start, a block opened there was "redrawn" on the primary against the
// alternate ring's row numbers, and a mark there stopped the *primary*
// re-cutting on a width change, because `Term::resize` read whichever
// screen had last written a mark. No recording carries a mark on the
// alternate screen -- a program that forwards an inner shell's marks, as a
// multiplexer can, is the shape -- so these are fixtures for what the
// corpus cannot contain, the HT and SO precedent.

/// Feed `bytes` in reads of `n`, so a test can ask the same question at a
/// read size of one and at the whole stream.
fn feed_in(t: &mut Term<'_>, bytes: &[u8], n: usize) {
    for chunk in bytes.chunks(n) {
        support::feed(t, chunk);
    }
}

/// The primary's history and screen as text: `hist` lines behind the
/// window, oldest first, then the screen's rows.
fn primary_text(t: &Term<'_>) -> (Vec<String>, Vec<String>) {
    let sb = t.scrollback();
    let hist = (0..sb.len())
        .rev()
        .map(|n| {
            let (c, _) = sb.line(Back::of(n)).expect("a retained line");
            text_of(c).trim_end().to_string()
        })
        .collect();
    let rows = t.geometry().1.index();
    (hist, (0..rows).map(|y| row_text(t, y)).collect())
}

/// Numbered lines, `from..to`, each ending in a newline.
fn numbered(tag: &str, from: usize, to: usize) -> Vec<u8> {
    (from..to)
        .flat_map(|i| format!("{tag} {i}\r\n").into_bytes())
        .collect()
}

/// A block opened on the primary does not reach the alternate screen: a
/// mark there opens that screen's own block rather than redrawing the
/// primary's by its ring row -- which erased five of the ten lines the
/// program drew -- and leaving restores the primary's block, which a redraw
/// then replaces where it is.
#[test]
fn a_mark_on_the_alternate_screen_does_not_redraw_the_primarys_block() {
    for n in [1, 3, 4096] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 20,
            },
            |t| {
                // The primary at a prompt, five rows down, with no `C`:
                // what a widget that opens a full-screen program from the
                // line editor leaves, and the zone the redraw keys on.
                let mut s = numbered("out", 0, 5);
                s.extend_from_slice(b"\x1b]133;P;k=i\x07$ ");
                feed_in(t, &s, n);
                assert_eq!(t.zone(), Zone::Prompt, "read size {n}");
                assert_eq!(t.prompt_row(), Some(Row::from_index(5)));
                // A full-screen program draws ten lines, and something in
                // it sends a prompt mark at row 10.
                let mut s = b"\x1b[?1049h".to_vec();
                s.extend_from_slice(&numbered("alt", 0, 10));
                s.extend_from_slice(b"\x1b]133;P;k=i\x07");
                feed_in(t, &s, n);
                let alt: Vec<String> = (0..11).map(|y| row_text(t, y)).collect();
                let want: Vec<String> = (0..10)
                    .map(|i| format!("alt {i}"))
                    .chain([String::new()])
                    .collect();
                assert_eq!(
                    alt, want,
                    "read size {n}: the alternate screen's text was erased by the primary's block"
                );
                assert_eq!(
                    t.prompt_erases, 0,
                    "read size {n}: the first mark there is fresh"
                );
                assert_eq!(t.cursor().0, Row::from_index(10), "read size {n}");
                assert_eq!(
                    t.prompt_row(),
                    Some(Row::from_index(10)),
                    "read size {n}: the alternate screen's own block"
                );
                // Leave, and the shell redraws the prompt it has.
                feed_in(t, b"\x1b[?1049l\r\x1b[J\x1b]133;P;k=i\x07$ ", n);
                assert_eq!(t.zone(), Zone::Prompt, "read size {n}");
                assert_eq!(
                    t.prompt_erases, 1,
                    "read size {n}: the redraw on the primary replaced the primary's block"
                );
                let (hist, screen) = primary_text(t);
                assert!(hist.is_empty(), "read size {n}: nothing scrolled");
                assert_eq!(
                    screen[..7],
                    ["out 0", "out 1", "out 2", "out 3", "out 4", "$", ""],
                    "read size {n}: the output above the block is untouched and one prompt stands"
                );
            },
        );
    }
}

/// The same mark once the primary has scrolled, so its block starts at a ring
/// row past the alternate screen's cursor row plus its ring: the redraw
/// subtracted the primary's ring
/// row from the alternate ring's position, which underflows -- a panic in a
/// debug build -- and in a release build wraps to a row count that erased
/// whatever it landed on.
#[test]
fn a_mark_on_the_alternate_screen_after_the_primary_scrolled() {
    for n in [1, 7, 4096] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 20,
            },
            |t| {
                // Sixty lines on twenty rows: the primary's ring has turned
                // forty-one rows and the block opens on the last row, ring
                // row sixty, past the alternate screen's five plus twenty.
                let mut s = numbered("line", 0, 60);
                s.extend_from_slice(b"\x1b]133;P;k=i\x07$ ");
                feed_in(t, &s, n);
                assert_eq!(t.prompt_row(), Some(Row::from_index(19)), "read size {n}");
                let mut s = b"\x1b[?1049h".to_vec();
                s.extend_from_slice(&numbered("alt", 0, 5));
                s.extend_from_slice(b"\x1b]133;P;k=i\x07");
                feed_in(t, &s, n);
                let alt: Vec<String> = (0..6).map(|y| row_text(t, y)).collect();
                assert_eq!(
                    alt,
                    ["alt 0", "alt 1", "alt 2", "alt 3", "alt 4", ""],
                    "read size {n}: the alternate screen's text"
                );
                assert_eq!(t.prompt_erases, 0, "read size {n}");
                assert_eq!(t.prompt_row(), Some(Row::from_index(5)), "read size {n}");
                // Back on the primary, the prompt it had is where it was.
                feed_in(t, b"\x1b[?1049l", n);
                assert_eq!(t.prompt_row(), Some(Row::from_index(19)), "read size {n}");
                assert_eq!(row_text(t, 18), "line 59", "read size {n}");
            },
        );
    }
}

/// The other direction of the same gate: the primary at a prompt, a program
/// taking the alternate screen from the line editor with no `C` -- a
/// full-screen fzf, `edit-command-line` -- and the window narrowed while it
/// runs. The primary's zone is the one parked, and it is `Prompt`, so its
/// prompt row is not re-cut; the zone the program's screen has is `Output`,
/// and reading that one re-cut it. `Term::primary_zone` answers the
/// question the resize asks, and `Term::zone` the live screen's.
#[test]
fn a_prompt_on_the_primary_stops_the_re_cut_while_a_program_is_up() {
    for n in [1, 4096] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 8,
            },
            |t| {
                let mut s = b"\x1b]133;P;k=i\x07".to_vec();
                s.extend_from_slice(long_line(36).as_bytes());
                feed_in(t, &s, n);
                let before = row_text(t, 0);
                assert_eq!(
                    before.len(),
                    36,
                    "read size {n}: the control, the prompt line"
                );
                feed_in(t, b"\x1b[?1049h", n);
                assert_eq!(
                    t.zone(),
                    Zone::Output,
                    "read size {n}: the program's screen"
                );
                assert_eq!(t.primary_zone(), Zone::Prompt, "read size {n}: the shell's");
                let _ = t.resize(&WinSize {
                    ws_col: 20,
                    ws_row: 8,
                });
                feed_in(t, b"\x1b[?1049l", n);
                assert_eq!(
                    row_text(t, 0),
                    before[..20],
                    "read size {n}: the prompt row keeps its layout"
                );
                assert!(
                    row_text(t, 1).is_empty(),
                    "read size {n}: the prompt was re-cut onto a second row: {:?}",
                    row_text(t, 1)
                );
            },
        );
    }
}

/// The alternate screen's prompt state starts fresh on each entry, as its
/// cells do: a block opened on one visit names rows the next visit's
/// clear emptied, so a mark on the second visit is a fresh prompt rather
/// than a redraw of the first visit's block.
#[test]
fn the_alternate_screens_prompt_state_starts_fresh_on_each_entry() {
    for n in [1, 4096] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 20,
            },
            |t| {
                feed_in(t, b"\x1b[?1049h\x1b[4;1H\x1b]133;P;k=i\x07$ \x1b[?1049l", n);
                assert_eq!(
                    t.zone(),
                    Zone::Output,
                    "read size {n}: the primary, untouched"
                );
                let mut s = b"\x1b[?1049h".to_vec();
                assert_eq!(t.prompt_erases, 0, "read size {n}");
                s.extend_from_slice(&numbered("alt", 0, 8));
                s.extend_from_slice(b"\x1b]133;P;k=i\x07");
                feed_in(t, &s, n);
                let alt: Vec<String> = (0..9).map(|y| row_text(t, y)).collect();
                let want: Vec<String> = (0..8)
                    .map(|i| format!("alt {i}"))
                    .chain([String::new()])
                    .collect();
                assert_eq!(
                    alt, want,
                    "read size {n}: the second visit's text was erased by the first visit's block"
                );
                assert_eq!(
                    t.prompt_erases, 0,
                    "read size {n}: a fresh prompt, not a redraw"
                );
                assert_eq!(t.prompt_row(), Some(Row::from_index(8)), "read size {n}");
            },
        );
    }
}

/// The TODO's second shape, and the one that reached the history: a block
/// opened on the alternate screen and left without a `C` was "redrawn" on
/// the primary against the alternate ring's row number -- nine lines of
/// the primary's history and its whole screen erased. The primary was in
/// `Zone::Output` when the program started, and that is what it returns
/// to.
#[test]
fn a_block_opened_on_the_alternate_screen_is_not_redrawn_on_the_primary() {
    for n in [1, 5, 4096] {
        support::with_corpus(|t| {
            // Sixty lines: eleven behind the window. A command runs.
            let mut s = numbered("line", 0, 60);
            s.extend_from_slice(b"\x1b]133;C\x07");
            feed_in(t, &s, n);
            assert_eq!(t.zone(), Zone::Output, "read size {n}");
            let before = primary_text(t);
            assert_eq!(
                before.0.len(),
                11,
                "read size {n}: the control, eleven retired"
            );
            // The program opens a block on its own screen and exits
            // without closing it.
            feed_in(
                t,
                b"\x1b[?1049h\x1b[3;1H\x1b]133;P;k=i\x07inner\x1b[?1049l",
                n,
            );
            let zone = t.zone();
            // A shell whose integration sends only the prompt mark: a
            // fresh prompt, not a redraw.
            feed_in(t, b"\x1b]133;P;k=i\x07$ ", n);
            let after = primary_text(t);
            assert_eq!(after.0, before.0, "read size {n}: the history is untouched");
            assert_eq!(
                after.1[..49],
                before.1[..49],
                "read size {n}: the screen above the prompt is untouched"
            );
            assert_eq!(after.1[49], "$", "read size {n}");
            assert_eq!(t.prompt_erases, 0, "read size {n}: nothing was replaced");
            assert_eq!(
                zone,
                Zone::Output,
                "read size {n}: the primary returns to the zone it left"
            );
        });
    }
}

/// A mark on the alternate screen does not decide whether the **primary**
/// re-cuts. `Term::resize` reflows the primary whichever screen is up, and
/// it asked the zone whichever screen had last written a mark -- so the
/// program's mark left the primary's long line cut at the narrow edge,
/// its tail hidden past it, where the same resize with no mark there
/// re-wraps it. Both arms are asked, and the control is the first.
#[test]
fn a_mark_on_the_alternate_screen_does_not_stop_the_primary_re_cutting() {
    for (mark, what) in [
        (&b""[..], "the control, no mark"),
        (
            &b"\x1b]133;P;k=i\x07"[..],
            "a prompt mark on the alternate screen",
        ),
    ] {
        for n in [1, 4096] {
            support::with_term(
                &WinSize {
                    ws_col: 40,
                    ws_row: 8,
                },
                |t| {
                    let mut s = b"above\r\n".to_vec();
                    s.extend_from_slice(long_line(36).as_bytes());
                    s.extend_from_slice(b"\r\n\x1b[?1049h");
                    s.extend_from_slice(mark);
                    feed_in(t, &s, n);
                    let _ = t.resize(&WinSize {
                        ws_col: 20,
                        ws_row: 8,
                    });
                    feed_in(t, b"\x1b[?1049l", n);
                    let line = long_line(36);
                    assert_eq!(row_text(t, 0), "above", "{what}, read size {n}");
                    assert_eq!(
                        row_text(t, 1),
                        line[..20],
                        "{what}, read size {n}: the head"
                    );
                    assert_eq!(
                        row_text(t, 2),
                        line[20..],
                        "{what}, read size {n}: the primary's line was not re-cut"
                    );
                },
            );
        }
    }
}

/// A full reset forgets what the marks said, from either screen: the shell
/// is at no prompt after one, and its block start names rows the reset
/// erased. So the next prompt mark is a fresh block, not a redraw of the old
/// one. The new lines put the cursor on row 8, below the stale start on
/// row 5, where a kept block start would be taken for a redraw and erase
/// `new 5` to `new 7` -- above it, the redraw refuses a start below the
/// cursor, and the behavioural half could not fail. foot's hard reset erases
/// every row's prompt marks (`[[source: foot-term-reset]]`); alacritty keeps
/// no prompt state.
#[test]
fn a_full_reset_forgets_the_prompt_from_either_screen() {
    for from_alt in [false, true] {
        support::with_term(
            &WinSize {
                ws_col: 40,
                ws_row: 20,
            },
            |t| {
                support::feed(t, &numbered("out", 0, 5));
                support::feed(t, b"\x1b]133;P;k=i\x07$ ");
                if from_alt {
                    support::feed(t, b"\x1b[?1049h\x1b]133;P;k=i\x07");
                }
                assert_eq!(t.zone(), Zone::Prompt, "alt {from_alt}: the control");
                support::feed(t, b"\x1bc");
                let (zone, row) = (t.zone(), t.prompt_row());
                support::feed(t, &numbered("new", 0, 8));
                support::feed(t, b"\x1b]133;P;k=i\x07$ ");
                assert_eq!(t.prompt_erases, 0, "alt {from_alt}: a fresh prompt");
                let rows: Vec<String> = (0..10).map(|y| row_text(t, y)).collect();
                let want: Vec<String> = (0..8)
                    .map(|i| format!("new {i}"))
                    .chain(["$".to_string(), String::new()])
                    .collect();
                assert_eq!(rows, want, "alt {from_alt}");
                assert_eq!(zone, Zone::Output, "alt {from_alt}");
                assert_eq!(row, None, "alt {from_alt}");
            },
        );
    }
}
