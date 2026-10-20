//! **`OSC 133`'s options, statuses and prompts**, read to their texts:
//! Per Bothner's semantic-prompts proposal (`[[source: semantic-prompts]]`)
//! for the letters, `aid`, `cl=`, `move-keys=` and `D`'s status; kitty's
//! shell-integration notes (`[[source: kitty-shell-integration]]`) for
//! `redraw=`, `click_events=` and `special_key=`; iTerm2's
//! (`[[source: iterm2-doc-escape-codes]]`) for a `D` with no `C`.
mod support;
use baud_core::{
    ClickEvents, ClickHow, ClickKeys, ExitCode, ExitStatus, PackedCell, PromptJump, PromptLine,
    Term, Zone,
};
use baud_domain::{Col, Row, Rows, WinSize};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

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

/// The text of the window's top row `back` lines into the history.
fn top_text(t: &Term<'_>, back: usize) -> String {
    text_of(t.view(Rows::of_count(back)).row(Row::ZERO))
        .trim_end()
        .to_string()
}

fn statuses(t: &Term<'_>) -> Vec<ExitStatus> {
    t.prompts().map(|p| p.status).collect()
}

fn code(n: i32) -> ExitStatus {
    ExitStatus::Exited(ExitCode::of(n))
}

/// **`L` and `A` start a line only where none is started**: "If the cursor
/// is the initial column ... do nothing. Otherwise, do the equivalent of
/// `\r\n`", and `A` does that first. `P` does not.
#[test]
fn a_fresh_line_starts_a_line_only_where_none_is_started() {
    support::with_term(&WS, |t| {
        support::feed(t, b"no newline\x1b]133;L\x07next");
        assert_eq!(row_text(t, 0), "no newline");
        assert_eq!(row_text(t, 1), "next");
        support::feed(t, b"\r\n\x1b]133;L\x07here");
        assert_eq!(row_text(t, 2), "here", "at the first column L does nothing");
        assert_eq!(row_text(t, 3), "");
        // bash prints its prompt right after output with no newline.
        support::feed(t, b" out\x1b]133;A\x07$ ");
        assert_eq!(row_text(t, 2), "here out");
        assert_eq!(
            row_text(t, 3),
            "$",
            "A began the prompt on a line of its own"
        );
        support::feed(t, b"\x1b]133;D\x07\r\n\x1b]133;P;k=i\x07% ");
        assert_eq!(t.cursor(), (Row::from_index(4), Col::at_index(2)));
        support::feed(t, b"x\x1b]133;P;k=r\x07R");
        assert_eq!(row_text(t, 4), "% xR", "P makes no fresh line");
        // `N` is `A` for this.
        support::feed(t, b"\x1b]133;D\x07\x1b]133;N;aid=9\x07> ");
        assert_eq!(row_text(t, 5), ">");
    });
}

/// **`redraw=0`: "the shell will not redraw the prompt on resize so it
/// should not erase it"**, and the next command's `A` without it asks for
/// the erase again: a setting belongs to the command whose `A` carried it.
#[test]
fn a_redraw_mark_under_redraw_0_erases_nothing() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]133;A;redraw=0\x07line one\r\nline two");
        support::feed(t, b"\x1b[1A\r\x1b]133;P;k=i\x07");
        assert_eq!(t.prompt_erases, 0, "the redraw mark erased under redraw=0");
        assert_eq!(
            row_text(t, 1),
            "line two",
            "the block's last row was erased"
        );
        support::feed(
            t,
            b"\x1b]133;D\x07\r\n\r\n\x1b]133;A\x07next one\r\nnext two",
        );
        support::feed(t, b"\x1b[1A\r\x1b]133;A\x07");
        assert_eq!(
            t.prompt_erases, 1,
            "an A without redraw= is the default again"
        );
        assert_eq!(row_text(t, 4), "", "the default redraw erases the block");
    });
}

/// **And under `redraw=0` a resize re-cuts the prompt like output**, as
/// kitty's resize re-wraps it: nothing will redraw it. The default prompt
/// is the control, which stays un-cut (`a_prompt_is_not_recut_by_a_resize`).
#[test]
fn a_prompt_under_redraw_0_is_recut_like_output() {
    for (mark, recut) in [
        (&b"\x1b]133;A;redraw=0\x07"[..], true),
        (b"\x1b]133;A\x07", false),
    ] {
        support::with_term(&WS, |t| {
            support::feed(t, b"out\r\n");
            support::feed(t, mark);
            support::feed(t, "a".repeat(36).as_bytes());
            assert_eq!(t.zone(), Zone::Prompt);
            let _ = t.resize(&WinSize {
                ws_col: 20,
                ws_row: 8,
            });
            assert_eq!(!row_text(t, 2).is_empty(), recut, "{mark:?}");
        });
    }
}

/// **`D`'s status stays on the prompt its command began with**: the code,
/// `err=` before it, and a `D` with no `C` the abort iTerm2 reads.
#[test]
fn d_keeps_its_status_on_the_prompt_it_ends() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07false\r\n\x1b]133;C\x07");
        support::feed(t, b"\x1b]133;D;1\x07");
        support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07true\r\n\x1b]133;C\x07");
        support::feed(t, b"\x1b]133;D;0;err=\x07");
        support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07^C\x1b]133;D;0\x07\r\n");
        support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07x\r\n\x1b]133;C\x07");
        support::feed(t, b"\x1b]133;D;0;err=ENOENT\x07");
        support::feed(t, b"\x1b]133;A\x07$ ");
        assert_eq!(
            statuses(t),
            [
                code(1),
                ExitStatus::Succeeded,
                ExitStatus::Cancelled,
                ExitStatus::Failed(None),
                ExitStatus::Unreported,
            ]
        );
    });
}

/// **A command nested in another takes its status by `aid`**: Ghostty's
/// bash integration sends `aid=$BASHPID` on its `A` and its `D`, so a bash
/// run from zsh is a command inside a command, and zsh's `D`, with no
/// `aid`, ends zsh's command past the bash prompt left open.
#[test]
fn a_nested_command_takes_its_status_by_aid() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]133;A\x07% \x1b]133;B\x07bash\r\n\x1b]133;C\x07");
        support::feed(
            t,
            b"\x1b]133;A;aid=77\x07$ \x1b]133;B\x07false\r\n\x1b]133;C\x07",
        );
        support::feed(t, b"\x1b]133;D;1;aid=77\x07");
        support::feed(t, b"\x1b]133;A;aid=77\x07$ \x1b]133;B\x07exit\r\n");
        support::feed(t, b"\x1b]133;D;0\x07");
        assert_eq!(statuses(t), [code(0), code(1), ExitStatus::Unreported]);
    });
}

/// **A redraw keeps one prompt**, at the line it now begins on: kitty's zsh
/// sends `A` in its prompt, so every redraw sends one, and Ghostty's bash
/// sends `A` then `P;k=i` for one prompt.
#[test]
fn a_redraw_keeps_one_prompt() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            b"\x1b]133;A;redraw=last;cl=line;aid=5\x07\x1b]133;P;k=i\x07$ ",
        );
        assert_eq!(t.prompts().count(), 1);
        support::feed(t, b"\r\x1b]133;A\x07$ ");
        assert_eq!(t.prompts().count(), 1);
        assert_eq!(t.prompt_erases, 2, "redraw=last erases as 1 does");
    });
}

/// Prompts in the history, three lines apart, each a command of one
/// output line.
fn history_of_prompts(t: &mut Term<'_>) {
    for n in 0..6 {
        let line = format!(
            "\x1b]133;A\x07$ cmd{n}\x1b]133;B\x07\r\n\x1b]133;C\x07out{n}\r\nmore{n}\r\n\x1b]133;D;0\x07"
        );
        support::feed(t, line.as_bytes());
    }
    support::feed(t, b"\x1b]133;A\x07$ ");
}

/// **A jump puts the previous or next prompt at the window's top**, as
/// kitty and Ghostty do, and a prompt on the live screen is the live
/// screen.
#[test]
fn a_jump_goes_to_the_previous_and_the_next_prompt() {
    support::with_term(&WS, |t| {
        history_of_prompts(t);
        assert_eq!(t.window_back(), Rows::ZERO);
        assert!(t.jump_to_prompt(PromptJump::Previous));
        let first = t.window_back();
        assert_eq!(top_text(t, first.index()), "$ cmd3");
        assert!(t.jump_to_prompt(PromptJump::Previous));
        assert_eq!(top_text(t, t.window_back().index()), "$ cmd2");
        assert!(t.jump_to_prompt(PromptJump::Next));
        assert_eq!(t.window_back(), first);
        assert!(
            t.jump_to_prompt(PromptJump::Next),
            "cmd4 is on the live screen"
        );
        assert_eq!(t.window_back(), Rows::ZERO);
        assert!(
            !t.jump_to_prompt(PromptJump::Next),
            "nothing past the live screen"
        );
        while t.jump_to_prompt(PromptJump::Previous) {}
        assert_eq!(top_text(t, t.window_back().index()), "$ cmd0");
        let lines: Vec<PromptLine> = t.prompts().map(|p| p.line).collect();
        assert_eq!(lines.len(), 7);
        assert!(matches!(lines[6], PromptLine::Screen(_)));
    });
}

/// **A re-cut moves each prompt with its line**: after a narrowing reflow
/// of the history, a jump still lands on the prompt's text.
#[test]
fn a_jump_lands_on_its_prompt_after_a_reflow() {
    support::with_term(&WS, |t| {
        // Lines before the prompt that the re-cut makes two rows each, so
        // the prompt's line moves from 3 to 6.
        for n in 0..3 {
            support::feed(t, format!("{}{n}\r\n", "e".repeat(30)).as_bytes());
        }
        support::feed(t, b"\x1b]133;A\x07$ ");
        support::feed(t, "x".repeat(30).as_bytes());
        support::feed(t, b"\x1b]133;B\x07\r\n\x1b]133;C\x07");
        for n in 0..10 {
            support::feed(t, format!("{}{n}\r\n", "o".repeat(30)).as_bytes());
        }
        support::feed(t, b"\x1b]133;D;0\x07");
        let _ = t.resize(&WinSize {
            ws_col: 16,
            ws_row: 8,
        });
        assert!(t.jump_to_prompt(PromptJump::Previous));
        assert_eq!(
            top_text(t, t.window_back().index()),
            format!("$ {}", "x".repeat(14))
        );
    });
}

/// **Erasing the history forgets its prompts**, and a full reset forgets
/// them all; the alternate screen keeps none.
#[test]
fn erasing_the_history_forgets_its_prompts() {
    support::with_term(&WS, |t| {
        history_of_prompts(t);
        let all = t.prompts().count();
        support::feed(t, b"\x1b[3J");
        let left = t.prompts().count();
        assert!(left < all && left > 0, "{left} of {all}: the screen's stay");
        assert!(!t.jump_to_prompt(PromptJump::Previous));
        support::feed(
            t,
            b"\x1b[?1049h\x1b]133;A\x07alt\x1b]133;D;0\x07\x1b[?1049l",
        );
        assert_eq!(t.prompts().count(), left, "the alternate screen keeps none");
        support::feed(t, b"\x1bc");
        assert_eq!(t.prompts().count(), 0);
    });
}

/// **A click at a prompt sends what the command's `A` asked**: kitty's
/// `click_events=` over `cl=`, `move-keys=` over `special_key=`, nothing
/// without either, and nothing once the command runs.
#[test]
fn a_click_sends_what_the_a_asked() {
    let cases: [(&[u8], Option<ClickHow>); 6] = [
        (b"A", None),
        (b"A;cl=line", Some(ClickHow::Keys(ClickKeys::Arrows))),
        (
            b"A;cl=w;special_key=1",
            Some(ClickHow::Keys(ClickKeys::Special)),
        ),
        (
            b"A;cl=m;click_events=1",
            Some(ClickHow::Report(ClickEvents::Absolute)),
        ),
        (
            b"A;click_events=2",
            Some(ClickHow::Report(ClickEvents::Relative)),
        ),
        (b"A;cl=line;move-keys=h,l", None),
    ];
    for (mark, want) in cases {
        support::with_term(&WS, |t| {
            support::feed(t, b"out\r\n\x1b]133;");
            support::feed(t, mark);
            support::feed(t, b"\x07$ \x1b]133;B\x07ls");
            let click = t.prompt_click();
            match want {
                Some(how) => assert_eq!(click.map(|c| c.how), Some(how), "{mark:?}"),
                None if mark.ends_with(b"h,l") => match click.map(|c| c.how) {
                    Some(ClickHow::Keys(ClickKeys::Sequences(k))) => {
                        assert_eq!((k.left(), k.right()), (&b"h"[..], &b"l"[..]));
                    }
                    other => panic!("{other:?}"),
                },
                None => assert_eq!(click, None, "{mark:?}"),
            }
            if let Some(c) = click {
                assert_eq!(c.top, Row::from_index(1));
                assert_eq!(c.input, Some((Row::from_index(1), Col::at_index(2))));
            }
            support::feed(t, b"\r\n\x1b]133;C\x07");
            assert_eq!(t.prompt_click(), None, "the command runs");
        });
    }
    // The next command's `A` asks afresh.
    support::with_term(&WS, |t| {
        support::feed(
            t,
            b"\x1b]133;A;cl=line\x07$ \x1b]133;D\x07\r\n\x1b]133;A\x07$ ",
        );
        assert_eq!(t.prompt_click(), None);
    });
}

/// **`I`'s input ends with its line**: output that follows with no `C`
/// is output, so a resize re-cuts it and its `D` is no abort.
#[test]
fn an_i_input_ends_with_its_line() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]133;A\x07> \x1b]133;I\x07(+ 1 2)\r\n");
        support::feed(t, "3".repeat(36).as_bytes());
        assert_eq!(t.zone(), Zone::Prompt, "nothing has said so yet");
        let _ = t.resize(&WinSize {
            ws_col: 20,
            ws_row: 8,
        });
        assert!(!row_text(t, 2).is_empty(), "the output was not re-cut");
        support::feed(t, b"\r\n\x1b]133;D;0\x07");
        assert_eq!(statuses(t), [code(0)]);
    });
}

/// **A full log forgets its oldest prompts, counted**, and a jump back
/// stops at the oldest it kept.
#[test]
fn a_full_log_forgets_its_oldest_prompts() {
    let mut owned = baud_core::Owned::new(&WS).with_prompt_log(baud_core::PromptCapacity::of(4));
    let mut t = Term::new(owned.storage(), &WS);
    history_of_prompts(&mut t);
    assert_eq!(t.prompts().count(), 4);
    assert_eq!(t.prompts_forgotten().index(), 3);
    assert!(t.jump_to_prompt(PromptJump::Previous));
    assert_eq!(top_text(&t, t.window_back().index()), "$ cmd3");
    assert!(
        !t.jump_to_prompt(PromptJump::Previous),
        "cmd2 was forgotten"
    );
}

/// **A clear takes the prompts it erased**: zsh's Control-L and `clear`
/// send `CSI H CSI 2 J`, and the prompt drawn at the top then is the only
/// one on the screen, so the log stays in line order and a jump finds the
/// prompts in the history, not ones the clear took.
#[test]
fn a_clear_takes_the_prompts_it_erased() {
    support::with_term(&WS, |t| {
        history_of_prompts(t);
        let before: Vec<PromptLine> = t.prompts().map(|p| p.line).collect();
        let on_screen = before
            .iter()
            .filter(|l| matches!(l, PromptLine::Screen(_)))
            .count();
        assert!(on_screen >= 2, "{before:?}");
        // Control-L at the prompt: the shell clears and redraws it.
        support::feed(t, b"\x1b[H\x1b[2J\x1b]133;A\x07$ ");
        let after: Vec<PromptLine> = t.prompts().map(|p| p.line).collect();
        assert_eq!(after.len(), before.len() - on_screen + 1, "{after:?}");
        assert_eq!(after.last(), Some(&PromptLine::Screen(Row::ZERO)));
        assert!(t.jump_to_prompt(PromptJump::Previous));
        assert!(top_text(t, t.window_back().index()).starts_with("$ cmd"));
        assert!(t.jump_to_prompt(PromptJump::Next));
        assert_eq!(t.window_back(), Rows::ZERO);
    });
}

/// **A prompt re-cut under `redraw=0` keeps its anchors on its cells**:
/// the block's first row and the input's first cell are ring rows, and a
/// re-cut moves the rows under them.
#[test]
fn a_recut_prompt_keeps_its_input_where_it_began() {
    support::with_term(&WS, |t| {
        for n in 0..3 {
            support::feed(t, format!("{}{n}\r\n", "e".repeat(30)).as_bytes());
        }
        support::feed(t, b"\x1b]133;A;redraw=0;cl=line\x07$ \x1b]133;B\x07");
        support::feed(t, "i".repeat(20).as_bytes());
        let _ = t.resize(&WinSize {
            ws_col: 16,
            ws_row: 8,
        });
        let click = t.prompt_click().expect("still at the prompt");
        let (row, col) = click.input.expect("the input on the screen");
        assert_eq!(click.top, row, "the input begins on the block's first row");
        let cell = t.row(row).expect("a row")[col.index()];
        assert_eq!(
            cell.content.as_codepoint().map(|c| c.value()),
            Some(u32::from('i'))
        );
        assert_eq!(row_text(t, row.index()), format!("$ {}", "i".repeat(14)));
    });
}

/// **A click after an `I` line ended is the program's**: the output has
/// begun, so no prompt asks for it.
#[test]
fn a_click_after_an_i_line_ended_is_no_prompts() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]133;A;cl=line\x07> \x1b]133;I\x07(+ 1 2)");
        assert!(t.prompt_click().is_some());
        support::feed(t, b"\r\n3");
        assert_eq!(t.prompt_click(), None);
    });
}
