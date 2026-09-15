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
use baud_core::{PackedCell, Term, Zone};
use baud_domain::{Row, WinSize};

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
fn a_taller_screen_does_not_reach_into_the_history_at_a_prompt() {
    // **The rows have to be retired while the screen is output's**, or
    // there is nothing for a growth to reach for: a shrink at a prompt
    // takes its rows off the bottom rather than the top, so it retires
    // nothing. What does retire is a shrink during a command's output --
    // and then the command ends, the shell draws a prompt, and a growth
    // that reached back would slide that prompt down the screen over
    // rows the shell is not going to redraw.
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
        // Now the command ends and the shell owns the screen.
        support::feed(t, b"\x1b]133;D\x07\x1b]133;P;k=i\x07$ ");
        assert_eq!(t.zone(), Zone::Prompt);
        let top_before = row_text(t, 0);
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: u16::try_from(rows.index()).unwrap(),
        });
        assert_eq!(
            row_text(t, 0),
            top_before,
            "the growth pulled history back over the top row while the shell owned the screen"
        );
        assert!(
            t.scrollback().len() >= retired,
            "the retired rows are still retained, so scrolling still reaches them"
        );
    });
}
